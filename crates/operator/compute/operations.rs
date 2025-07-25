use k8s_openapi::api::core::v1::Pod;
use kube::api::{Api, AttachParams, ListParams};
use tracing::info;

/// Sends SIGHUP to postgres process in compute pod after verifying ConfigMap sync
pub async fn send_sighup_to_compute(
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
