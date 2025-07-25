use chrono;
use k8s_openapi::{
    api::{
        apps::v1::Deployment,
        core::v1::{ConfigMap, Pod},
    },
    apimachinery::pkg::apis::meta::v1::ObjectMeta,
    serde_json::json,
};
use kube::{
    api::{Api, ListParams, Patch, PatchParams},
    Client,
};
use tracing::info;

use crate::compute::{operations::send_sighup_to_compute, spec::update_compute_spec_json};

/// Service for processing compute hook notifications
pub struct HookService {
    client: Client,
}

impl HookService {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    /// Process a notify-attach hook request for a specific tenant
    pub async fn process_notify_attach(
        &self,
        tenant_id: &str,
        stripe_size: Option<u32>,
        shards: &[ComputeHookNotifyRequestShard],
    ) -> Result<String, String> {
        info!("Processing notify-attach request for tenant {}", tenant_id);

        // The compute pods are deployed in the "default" namespace
        let namespace = "default";

        // Find compute pods for this tenant
        let deployments: Api<Deployment> = Api::namespaced(self.client.clone(), namespace);
        let config_maps: Api<ConfigMap> = Api::namespaced(self.client.clone(), namespace);
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
                    if project_tenant_id == tenant_id {
                        // Found a compute deployment for this tenant
                        let deployment_name = deployment.metadata.name.as_ref().unwrap();
                        info!(
                            "Found compute deployment {} for tenant {}",
                            deployment_name, tenant_id
                        );

                        // Build the pageserver connection string from shards
                        let pageserver_connstring = self.build_pageserver_connstring(shards);
                        info!(
                            "Building new pageserver connection string: {}",
                            pageserver_connstring
                        );

                        // Update the ConfigMap and trigger pod resync
                        self.update_compute_config(
                            &config_maps,
                            &pods,
                            deployment_name,
                            &pageserver_connstring,
                            stripe_size,
                            namespace,
                        )
                        .await?;

                        // Send SIGHUP to the compute pod
                        send_sighup_to_compute(&pods, deployment_name, &pageserver_connstring)
                            .await
                            .map_err(|e| {
                                format!("Failed to send SIGHUP to deployment {}: {}", deployment_name, e)
                            })?;

                        info!("Successfully sent SIGHUP to deployment {}", deployment_name);
                        updated_count += 1;
                    }
                }
            }
        }

        if updated_count == 0 {
            return Err(format!(
                "No compute pods matching tenant ID {} were found to update",
                tenant_id
            ));
        }

        Ok(format!(
            "Updated {} compute pod(s) for tenant {}",
            updated_count, tenant_id
        ))
    }

    /// Build pageserver connection string from shards
    fn build_pageserver_connstring(&self, shards: &[ComputeHookNotifyRequestShard]) -> String {
        let pageserver_hosts: Vec<String> = shards
            .iter()
            .map(|shard| {
                format!(
                    "host=pageserver-{}.neon.svc.cluster.local port=6400",
                    shard.node_id
                )
            })
            .collect();
        pageserver_hosts.join(",")
    }

    /// Update compute ConfigMap and trigger pod resync
    async fn update_compute_config(
        &self,
        config_maps: &Api<ConfigMap>,
        pods: &Api<Pod>,
        deployment_name: &str,
        pageserver_connstring: &str,
        stripe_size: Option<u32>,
        namespace: &str,
    ) -> Result<(), String> {
        // Find and update the ConfigMap
        // The configmap name pattern: {branch-name}-compute-spec
        // The deployment name pattern: {branch-name}-compute-node
        let branch_name = deployment_name
            .strip_suffix("-compute-node")
            .unwrap_or(deployment_name);
        let configmap_name = format!("{}-compute-spec", branch_name);

        let existing_configmap = config_maps
            .get(&configmap_name)
            .await
            .map_err(|e| format!("Failed to get ConfigMap {}: {}", configmap_name, e))?;

        // Get the current spec.json data
        if let Some(data) = existing_configmap.data.as_ref() {
            if let Some(spec_json) = data.get("spec.json") {
                // Parse and update the JSON
                let updated_json = update_compute_spec_json(spec_json, pageserver_connstring, stripe_size)
                    .map_err(|e| format!("Failed to update compute spec for tenant: {}", e))?;

                // Create a new ConfigMap with only the necessary fields
                let updated_configmap = ConfigMap {
                    metadata: ObjectMeta {
                        name: Some(configmap_name.clone()),
                        namespace: Some(namespace.to_string()),
                        ..Default::default()
                    },
                    data: Some([("spec.json".to_string(), updated_json)].into_iter().collect()),
                    ..Default::default()
                };

                // Apply the updated ConfigMap
                config_maps
                    .patch(
                        &configmap_name,
                        &PatchParams::apply("kube-rs-controller"),
                        &Patch::Apply(&updated_configmap),
                    )
                    .await
                    .map_err(|e| format!("Failed to update ConfigMap {}: {}", configmap_name, e))?;

                info!(
                    "Successfully updated ConfigMap {} with new pageserver connection string",
                    configmap_name
                );

                // Update pod annotation to trigger ConfigMap resync
                self.update_pod_annotation(pods, deployment_name).await?;

                Ok(())
            } else {
                Err("No spec.json found in ConfigMap data".to_string())
            }
        } else {
            Err("No data found in ConfigMap".to_string())
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
}

/// Shard information for compute hook notification
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct ComputeHookNotifyRequestShard {
    pub node_id: u64,
    pub shard_number: u32,
}
