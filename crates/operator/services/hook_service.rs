use std::collections::BTreeMap;

use chrono::{self, Duration};
use k8s_openapi::{
    api::{
        apps::v1::Deployment,
        core::v1::{Pod, Secret, Service},
    },
    serde_json::{self, json},
};
use kube::{
    api::{Api, ListParams, Patch, PatchParams},
    Client,
};
use neon_cluster::{
    compute::{
        generate_compute_spec,
        spec::{
            extract_cluster_name, find_compute_deployment, ComputeHookNotifyRequest,
            ComputeHookNotifyRequestShard,
        },
    },
    util::secrets::{get_jwt_keys_from_secret, get_key_pair_from_secret},
};
use tokio::io::DuplexStream;
use tracing::{error, info};

use crate::{
    compute::operations::{exec_write_file_to_pod, send_sighup_to_compute},
    handlers::compute,
};

/// Service for processing compute hook notifications
pub struct HookService {
    client: Client,
}

impl HookService {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    /// Process a notify-attach hook request for a specific tenant
    pub async fn process_notify_attach(&self, request: &ComputeHookNotifyRequest) -> Result<String, String> {
        info!(
            "Processing notify-attach request for tenant {}",
            request.tenant_id
        );

        // The compute pods are deployed in the "default" namespace
        let namespace = "default";

        // Find compute pods for this tenant
        let deployments: Api<Deployment> = Api::namespaced(self.client.clone(), namespace);
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), namespace);

        // List all deployments and find ones for this tenant
        let deployment_list = deployments
            .list(&ListParams::default())
            .await
            .map_err(|e| format!("Failed to list deployments in namespace {}: {}", namespace, e))?;

        // Find deployments that belong to this tenant
        let mut updated_count = 0;
        for deployment in deployment_list {
            let labels = deployment.metadata.labels.as_ref();
            if let Some(labels) = labels {
                // Check if this deployment belongs to the tenant
                if let Some(project_tenant_id) = labels.get("neon.tenant_id") {
                    if project_tenant_id == &request.tenant_id {
                        // Found a compute deployment for this tenant
                        let deployment_name = deployment.metadata.name.as_ref().unwrap();
                        info!(
                            "Found compute deployment {} for tenant {}",
                            deployment_name, request.tenant_id
                        );
                        let compute_name = deployment_name
                            .strip_suffix("-compute-node")
                            .unwrap_or(deployment_name);

                        // Refresh configuration which triggers the compute to attach to the new pageserver
                        self.refresh_configuration(request, compute_name).await?;
                        updated_count += 1;
                    }
                }
            }
        }

        if updated_count == 0 {
            return Err(format!(
                "No compute pods matching tenant ID {} were found to update",
                request.tenant_id
            ));
        }

        Ok(format!(
            "Updated {} compute pod(s) for tenant {}",
            updated_count, request.tenant_id
        ))
    }

    /// Write compute spec directly to container file and trigger pod resync
    async fn write_compute_spec_to_container(
        &self,
        request: &ComputeHookNotifyRequest,
        pods: &Api<Pod>,
        deployment_name: &str,
    ) -> Result<(), String> {
        // Extract compute_name from deployment name
        // The deployment name pattern: {branch-name}-compute-node
        let compute_name = deployment_name
            .strip_suffix("-compute-node")
            .unwrap_or(deployment_name);

        // Generate the compute spec using the function from spec.rs
        let spec_json = generate_compute_spec(&self.client, Some(request), compute_name)
            .await
            .map_err(|e| format!("Failed to generate compute spec for {}: {}", compute_name, e))?;

        let spec_json_string = serde_json::to_string_pretty(&spec_json)
            .map_err(|e| format!("Failed to serialize compute spec: {}", e))?;

        // Find the pod for this deployment
        let pod_list = pods
            .list(&ListParams::default().labels(&format!("app={}", deployment_name)))
            .await
            .map_err(|e| format!("Failed to list pods for deployment {}: {}", deployment_name, e))?;

        if let Some(pod) = pod_list.items.first() {
            if let Some(pod_name) = &pod.metadata.name {
                info!("Writing compute spec to /var/spec.json in pod {}", pod_name);

                // Execute command to write spec.json to /var/spec.json in the container
                exec_write_file_to_pod(pods, pod_name, &spec_json_string)
                    .await
                    .map_err(|e| format!("Failed to write spec to pod {}: {}", pod_name, e))?;

                // Update pod annotation to trigger resync
                self.update_pod_annotation(pods, deployment_name).await?;

                Ok(())
            } else {
                Err("Pod name not found".to_string())
            }
        } else {
            Err(format!("No pods found for deployment {}", deployment_name))
        }
    }

    /// Update pod annotation to trigger ConfigMap resync
    async fn update_pod_annotation(&self, pods: &Api<Pod>, deployment_name: &str) -> Result<(), String> {
        let pod_list = pods
            .list(&ListParams::default().labels(&format!("app={}", deployment_name)))
            .await
            .map_err(|e| format!("Failed to list pods for deployment {}: {}", deployment_name, e))?;

        if let Some(pod) = pod_list.items.first() {
            if let Some(pod_name) = &pod.metadata.name {
                // Update annotation to trigger ConfigMap resync
                let timestamp = chrono::Utc::now().to_rfc3339();
                let patch = json!({
                    "metadata": {
                        "annotations": {
                            "last-notify-attach-hook": timestamp
                        }
                    }
                });

                pods.patch(pod_name, &PatchParams::default(), &Patch::Merge(patch))
                    .await
                    .map_err(|e| format!("Failed to update pod annotation for {}: {}", pod_name, e))?;

                info!("Updated pod {} annotation to trigger ConfigMap resync", pod_name);
                Ok(())
            } else {
                Err("Pod name not found".to_string())
            }
        } else {
            Ok(()) // No pods found, but this isn't an error condition
        }
    }

    async fn refresh_configuration(
        &self,
        request: &ComputeHookNotifyRequest,
        compute_name: &str,
    ) -> Result<(), String> {
        // Get services matching tenant ID and call /configure with the compute spec as body

        let spec_json = generate_compute_spec(&self.client, Some(request), compute_name)
            .await
            .map_err(|e| format!("Failed to generate compute spec for {}: {}", compute_name, e))?;

        let services: Api<Service> = Api::all(self.client.clone());

        let deployment = match find_compute_deployment(&self.client, compute_name).await {
            Ok(d) => d,
            Err(e) => {
                error!(
                    "Failed to find compute deployment for compute_id {}: {}",
                    compute_name, e
                );
                return Err(format!(
                    "Failed to find compute deployment for compute_id {}: {}",
                    compute_name, e
                ));
            }
        };

        let cluster_name = match extract_cluster_name(&deployment) {
            Ok(name) => name,
            Err(e) => {
                error!("Failed to extract cluster name from deployment: {}", e);
                return Err(format!("Failed to extract cluster name from deployment: {}", e));
            }
        };

        let key_pair = get_key_pair_from_secret(&self.client, cluster_name.as_str())
            .await
            .map_err(|e| format!("Failed to get key pair for cluster {}: {}", cluster_name, e))?;

        let mut claims = BTreeMap::new();
        claims.insert("compute_id".to_string(), compute_name.to_string());

        let token = key_pair
            .generate_jwt_token(
                Duration::new(3600, 0).unwrap(),
                None,
                None,
                vec!["compute".to_string()],
                vec!["compute_ctl:admin".to_string()],
                Some(claims),
            )
            .unwrap();

        let services_list = services
            .list(&ListParams {
                label_selector: Some(format!("neon.tenant_id={}", request.tenant_id)),
                ..Default::default()
            })
            .await
            .map_err(|e| format!("Failed to list services for tenant {}: {}", request.tenant_id, e))?;

        for service in services_list.items {
            let service_name = service.metadata.name.as_ref().unwrap();
            let service_namespace = service.metadata.namespace.as_ref().unwrap();
            let url = format!(
                "http://{}-admin.{}:3080/configure",
                compute_name, service_namespace
            );

            info!("Calling /configure endpoint at URL: {}", url);
            info!(
                "Spec JSON size: {} bytes",
                serde_json::to_string(&spec_json).unwrap_or_default().len()
            );

            let response = reqwest::Client::new()
                .post(&url)
                .json(&spec_json)
                .header("Authorization", format!("Bearer {}", token))
                .timeout(std::time::Duration::from_secs(2))
                .send()
                .await
                .map_err(|e| {
                    error!(
                        "Failed to call /configure for service {} at URL {}: {:?}",
                        service_name, url, e
                    );
                    format!("Failed to call /configure for service {}: {:?}", service_name, e)
                })?;

            if !response.status().is_success() {
                error!(
                    "Failed to call /configure for service {}: {}",
                    service_name,
                    response.status()
                );
                return Err(format!(
                    "Failed to call /configure for service {}: {}",
                    service_name,
                    response.status()
                ));
            }
        }

        Ok(())
    }
}
