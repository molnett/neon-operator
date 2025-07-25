#![allow(unused_imports, unused_variables)]
use actix_web::{
    get, middleware, put,
    web::{Data, Json},
    App, HttpRequest, HttpResponse, HttpServer, Responder,
};
use chrono;
use serde::{Deserialize, Serialize};

use k8s_openapi::{
    api::{
        apps::v1::Deployment,
        core::v1::{ConfigMap, Pod},
    },
    apimachinery::pkg::apis::meta::v1::ObjectMeta,
};
use kube::{
    api::{Api, AttachParams, ListParams, Patch, PatchParams},
    Client,
};
use neon_cluster::controllers;
use std::collections::BTreeMap;

use neon_cluster::compute::spec::generate_compute_spec;
use neon_cluster::util::telemetry;

use k8s_openapi::serde_json::{json, Value};
use prometheus::{Encoder, TextEncoder};
use tracing::{error, info};

#[get("/metrics")]
async fn metrics(
    c: Data<neon_cluster::controllers::cluster_controller::State>,
    _req: HttpRequest,
) -> impl Responder {
    let metrics = c.metrics();
    let encoder = TextEncoder::new();
    let mut buffer = vec![];
    encoder.encode(&metrics, &mut buffer).unwrap();
    HttpResponse::Ok().body(buffer)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct ComputeHookNotifyRequestShard {
    node_id: u64,
    shard_number: u32,
}

/// Request body that we send to the control plane to notify it of where a tenant is attached
#[derive(Serialize, Deserialize, Debug)]
struct ComputeHookNotifyRequest {
    tenant_id: String,
    stripe_size: Option<u32>,
    shards: Vec<ComputeHookNotifyRequestShard>,
}

#[put("/notify-attach")]
async fn notify_attach(
    _c: Data<neon_cluster::controllers::cluster_controller::State>,
    req_body: Json<ComputeHookNotifyRequest>,
) -> impl Responder {
    let client = match Client::try_default().await {
        Ok(client) => client,
        Err(e) => {
            error!("Failed to create kube client: {}", e);
            return HttpResponse::InternalServerError().json(format!("Failed to create kube client: {}", e));
        }
    };

    info!("Received notify-attach request for tenant {}", req_body.tenant_id);

    let tenant_id = &req_body.tenant_id;

    // The compute pods are deployed in the "default" namespace
    let namespace = "default";

    // Find compute pods for this tenant
    let deployments: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    let config_maps: Api<ConfigMap> = Api::namespaced(client.clone(), namespace);
    let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);

    // List all deployments and find ones for this tenant
    let deployment_list = match deployments.list(&ListParams::default()).await {
        Ok(list) => list,
        Err(e) => {
            error!("Failed to list deployments in namespace {}: {}", namespace, e);
            return HttpResponse::InternalServerError().json(format!("Failed to list deployments: {}", e));
        }
    };

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
                    let mut pageserver_hosts = Vec::new();
                    for shard in &req_body.shards {
                        let host = format!(
                            "host=pageserver-{}.neon.svc.cluster.local port=6400",
                            shard.node_id
                        );
                        pageserver_hosts.push(host);
                    }
                    let pageserver_connstring = pageserver_hosts.join(",");
                    info!(
                        "Building new pageserver connection string: {}",
                        pageserver_connstring
                    );

                    // Find and update the ConfigMap
                    // The configmap name pattern: {branch-name}-compute-spec
                    // The deployment name pattern: {branch-name}-compute-node
                    let branch_name = deployment_name
                        .strip_suffix("-compute-node")
                        .unwrap_or(deployment_name);
                    let configmap_name = format!("{}-compute-spec", branch_name);
                    match config_maps.get(&configmap_name).await {
                        Ok(existing_configmap) => {
                            // Get the current spec.json data
                            if let Some(data) = existing_configmap.data.as_ref() {
                                if let Some(spec_json) = data.get("spec.json") {
                                    // Parse and update the JSON
                                    match update_compute_spec_json(
                                        spec_json,
                                        &pageserver_connstring,
                                        req_body.stripe_size,
                                    ) {
                                        Ok(updated_json) => {
                                            // Create a new ConfigMap with only the necessary fields
                                            let updated_configmap = ConfigMap {
                                                metadata: ObjectMeta {
                                                    name: Some(configmap_name.clone()),
                                                    namespace: Some(namespace.to_string()),
                                                    ..Default::default()
                                                },
                                                data: Some(
                                                    [("spec.json".to_string(), updated_json)]
                                                        .into_iter()
                                                        .collect(),
                                                ),
                                                ..Default::default()
                                            };

                                            // Apply the updated ConfigMap
                                            match config_maps
                                                .patch(
                                                    &configmap_name,
                                                    &PatchParams::apply("kube-rs-controller"),
                                                    &Patch::Apply(&updated_configmap),
                                                )
                                                .await
                                            {
                                                Ok(_) => {
                                                    info!("Successfully updated ConfigMap {} with new pageserver connection string", configmap_name);

                                                    // Update pod annotation to trigger ConfigMap resync
                                                    let pod_list = match pods
                                                        .list(
                                                            &ListParams::default()
                                                                .labels(&format!("app={}", deployment_name)),
                                                        )
                                                        .await
                                                    {
                                                        Ok(list) => list,
                                                        Err(e) => {
                                                            error!(
                                                                "Failed to list pods for deployment {}: {}",
                                                                deployment_name, e
                                                            );
                                                            return HttpResponse::InternalServerError()
                                                                .json(format!("Failed to list pods: {}", e));
                                                        }
                                                    };

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

                                                            match pods
                                                                .patch(
                                                                    pod_name,
                                                                    &PatchParams::default(),
                                                                    &Patch::Merge(patch),
                                                                )
                                                                .await
                                                            {
                                                                Ok(_) => {}
                                                                Err(e) => {
                                                                    error!("Failed to update pod annotation for {}: {}", pod_name, e);
                                                                    return HttpResponse::InternalServerError()
                                                                        .json(format!("Failed to update pod annotation: {}", e));
                                                                }
                                                            }

                                                            info!("Updated pod {} annotation to trigger ConfigMap resync", pod_name);
                                                        }
                                                    }

                                                    // Send SIGHUP to the compute pod
                                                    if let Err(e) = send_sighup_to_compute(
                                                        &pods,
                                                        deployment_name,
                                                        &pageserver_connstring,
                                                    )
                                                    .await
                                                    {
                                                        error!(
                                                            "Failed to send SIGHUP to deployment {}: {}",
                                                            deployment_name, e
                                                        );
                                                        return HttpResponse::InternalServerError()
                                                            .json(format!("Failed to send SIGHUP: {}", e));
                                                    }
                                                    info!(
                                                        "Successfully sent SIGHUP to deployment {}",
                                                        deployment_name
                                                    );
                                                    updated_count += 1;
                                                }
                                                Err(e) => {
                                                    error!(
                                                        "Failed to update ConfigMap {}: {}",
                                                        configmap_name, e
                                                    );
                                                    return HttpResponse::InternalServerError()
                                                        .json(format!("Failed to update ConfigMap: {}", e));
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            error!(
                                                "Failed to update compute spec for tenant {}: {}",
                                                tenant_id, e
                                            );
                                            return HttpResponse::InternalServerError()
                                                .json(format!("Failed to update compute spec: {}", e));
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to get ConfigMap {}: {}", configmap_name, e);
                            return HttpResponse::InternalServerError()
                                .json(format!("Failed to get ConfigMap: {}", e));
                        }
                    }
                }
            }
        }
    }

    if updated_count == 0 {
        return HttpResponse::NotFound().json(format!(
            "No compute pods matching tenant ID {} were found to update",
            tenant_id
        ));
    }

    HttpResponse::Ok().json(format!(
        "Updated {} compute pod(s) for tenant {}",
        updated_count, tenant_id
    ))
}

fn update_compute_spec_json(
    spec_json: &str,
    pageserver_connstring: &str,
    stripe_size: Option<u32>,
) -> Result<String, String> {
    let mut spec: Value = k8s_openapi::serde_json::from_str(spec_json)
        .map_err(|e| format!("Failed to parse spec.json: {}", e))?;

    // Get mutable reference to settings array, fail if structure is invalid
    let cluster = spec["spec"]["cluster"]
        .as_object_mut()
        .ok_or("spec.cluster is not an object")?;

    let settings = cluster["settings"]
        .as_array_mut()
        .ok_or("spec.cluster.settings is not an array")?;

    // Update neon.pageserver_connstring - must find and update it
    let mut found_pageserver_connstring = false;
    for setting in settings.iter_mut() {
        let name = setting["name"].as_str().ok_or("setting.name is not a string")?;

        if name == "neon.pageserver_connstring" {
            setting["value"] = json!(pageserver_connstring);
            found_pageserver_connstring = true;
        } else if name == "neon.shard_stripe_size" && stripe_size.is_some() {
            setting["value"] = json!(stripe_size.unwrap().to_string());
        }
    }

    if !found_pageserver_connstring {
        return Err("neon.pageserver_connstring setting not found in compute spec".to_string());
    }

    // Add neon.shard_stripe_size if it doesn't exist and stripe_size is provided
    if let Some(stripe_size) = stripe_size {
        let has_stripe_size = settings
            .iter()
            .any(|s| s["name"].as_str() == Some("neon.shard_stripe_size"));

        if !has_stripe_size {
            settings.push(json!({
                "name": "neon.shard_stripe_size",
                "value": stripe_size.to_string(),
                "vartype": "integer"
            }));
        }
    }

    k8s_openapi::serde_json::to_string_pretty(&spec)
        .map_err(|e| format!("Failed to serialize updated spec: {}", e))
}

async fn send_sighup_to_compute(
    pods: &Api<Pod>,
    deployment_name: &str,
    expected_pageserver_connstring: &str,
) -> Result<(), kube::Error> {
    // Find the pod for this deployment
    let pod_list = pods
        .list(&ListParams::default().labels(&format!("app={}", deployment_name)))
        .await?;

    if let Some(pod) = pod_list.items.first() {
        if let Some(pod_name) = &pod.metadata.name {
            // First, verify that the ConfigMap has been synced to the pod
            // Use grep and cut to extract the pageserver_connstring value since jq is not available
            let check_command = [
                "sh",
                "-c",
                "grep -A1 '\"neon.pageserver_connstring\"' /var/spec.json | grep '\"value\"' | cut -d \" -f4",
            ];

            let attach_params = AttachParams {
                container: None,
                tty: false,
                stdin: false,
                stdout: true,
                stderr: true,
                max_stdin_buf_size: None,
                max_stdout_buf_size: None,
                max_stderr_buf_size: None,
            };

            let mut attached = pods.exec(pod_name, check_command, &attach_params).await?;

            // Read stdout to get the current pageserver_connstring
            let mut stdout = String::new();
            if let Some(mut stdout_reader) = attached.stdout() {
                use tokio::io::AsyncReadExt;
                stdout_reader
                    .read_to_string(&mut stdout)
                    .await
                    .unwrap_or_default();
            }

            let status = attached.take_status().unwrap().await;

            if let Some(exit_status) = status {
                if exit_status.code == Some(0) {
                    let current_connstring = stdout.trim();
                    if current_connstring != expected_pageserver_connstring {
                        // ConfigMap not synced yet, fail and let the controller retry
                        return Err(kube::Error::Api(kube::error::ErrorResponse {
                            status: "500".to_string(),
                            message: format!(
                                "ConfigMap not synced to pod yet. Current: '{}', Expected: '{}'",
                                current_connstring, expected_pageserver_connstring
                            ),
                            reason: "ConfigNotSynced".to_string(),
                            code: 500,
                        }));
                    }
                    info!(
                        "ConfigMap synced successfully, current pageserver_connstring: {}",
                        current_connstring
                    );
                } else {
                    return Err(kube::Error::Api(kube::error::ErrorResponse {
                        status: "500".to_string(),
                        message: "Failed to read current configuration from pod".to_string(),
                        reason: "ConfigReadFailed".to_string(),
                        code: 500,
                    }));
                }
            }

            // Now send SIGHUP to postgres process
            let command = ["sh", "-c", "pkill -HUP postgres"];

            let attach_params = AttachParams {
                container: None,
                tty: false,
                stdin: false,
                stdout: true,
                stderr: true,
                max_stdin_buf_size: None,
                max_stdout_buf_size: None,
                max_stderr_buf_size: None,
            };

            let mut attached = pods.exec(pod_name, command, &attach_params).await?;
            let status = attached.take_status().unwrap().await;

            // Check if the command was successful
            if let Some(exit_status) = status {
                if let Some(code) = exit_status.code {
                    if code != 0 {
                        return Err(kube::Error::Api(kube::error::ErrorResponse {
                            status: "500".to_string(),
                            message: format!("Failed to send SIGHUP to postgres: exit code {}", code),
                            reason: "CommandFailed".to_string(),
                            code: 500,
                        }));
                    }
                }
            }

            Ok(())
        } else {
            Err(kube::Error::Api(kube::error::ErrorResponse {
                status: "404".to_string(),
                message: "Pod name not found".to_string(),
                reason: "NotFound".to_string(),
                code: 404,
            }))
        }
    } else {
        Err(kube::Error::Api(kube::error::ErrorResponse {
            status: "404".to_string(),
            message: format!("No pod found for deployment {}", deployment_name),
            reason: "NotFound".to_string(),
            code: 404,
        }))
    }
}

#[get("/compute/api/v2/computes/{compute_id}/spec")]
async fn compute_spec(compute_id: actix_web::web::Path<String>) -> impl Responder {
    let client = match Client::try_default().await {
        Ok(client) => client,
        Err(e) => {
            error!("Failed to create kube client: {}", e);
            return HttpResponse::InternalServerError().json(format!("Failed to create kube client: {}", e));
        }
    };

    info!(
        "Received compute spec request for compute_id: {}",
        compute_id.as_str()
    );

    match generate_compute_spec(&client, compute_id.as_str()).await {
        Ok(spec) => HttpResponse::Ok().json(spec),
        Err(e) => {
            error!("Failed to generate compute spec: {}", e);
            HttpResponse::InternalServerError().json(format!("Failed to generate compute spec: {}", e))
        }
    }
}

#[get("/health")]
async fn health(_: HttpRequest) -> impl Responder {
    HttpResponse::Ok().json("healthy")
}

#[get("/")]
async fn index(
    c: Data<neon_cluster::controllers::cluster_controller::State>,
    _req: HttpRequest,
) -> impl Responder {
    let d = c.diagnostics().await;
    HttpResponse::Ok().json(&d)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    telemetry::init().await;

    // Initiatilize Kubernetes controller state
    let state = neon_cluster::controllers::cluster_controller::State::default();
    let project_state = neon_cluster::controllers::project_controller::State::default();
    let branch_state = neon_cluster::controllers::branch_controller::State::default();
    let neon_cluster_controller = neon_cluster::controllers::cluster_controller::run(state.clone());
    let neon_project_controller = neon_cluster::controllers::project_controller::run(project_state.clone());
    let neon_branch_controller = neon_cluster::controllers::branch_controller::run(branch_state.clone());

    // Start web server
    let server = HttpServer::new(move || {
        App::new()
            .app_data(Data::new(state.clone()))
            .wrap(middleware::Logger::default().exclude("/health"))
            .service(index)
            .service(health)
            .service(metrics)
            .service(notify_attach)
            .service(compute_spec)
    })
    .bind("0.0.0.0:8080")?
    .shutdown_timeout(5);

    // Both runtimes implements graceful shutdown, so poll until both are done
    tokio::join!(
        neon_cluster_controller,
        neon_project_controller,
        neon_branch_controller,
        server.run()
    )
    .3?;
    Ok(())
}
