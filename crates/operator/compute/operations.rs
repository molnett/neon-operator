use std::process::exit;

use k8s_openapi::api::core::v1::Pod;
use kube::api::{Api, AttachParams, ListParams};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::info;

/// Sends SIGHUP to postgres process in compute pod after verifying ConfigMap sync
pub async fn send_sighup_to_compute(pods: &Api<Pod>, deployment_name: &str) -> Result<(), kube::Error> {
    // Find the pod for this deployment
    let pod_list = pods
        .list(&ListParams::default().labels(&format!("app={}", deployment_name)))
        .await?;

    if let Some(pod) = pod_list.items.first() {
        if let Some(pod_name) = &pod.metadata.name {
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

/// Execute command to write file content to a pod container
pub async fn exec_write_file_to_pod(
    pods: &Api<Pod>,
    pod_name: &str,
    content: &str,
) -> Result<(), kube::Error> {
    // Ensure the target directory exists
    let prep_cmd = ["sh", "-c", "mkdir -p /var"];
    let mut prep_attach = pods.exec(pod_name, prep_cmd, &AttachParams::default()).await?;
    prep_attach.take_status().unwrap().await;

    // Write content directly to the file
    let cmd = ["sh", "-c", "cat > /var/spec.json"];

    let attach_params = AttachParams {
        container: None,
        tty: false,
        stdin: true,
        stdout: true,
        stderr: true,
        max_stdin_buf_size: None,
        max_stdout_buf_size: None,
        max_stderr_buf_size: None,
    };

    let mut attach = pods.exec(pod_name, cmd, &attach_params).await?;

    // Write content to stdin and close it
    if let Some(mut writer) = attach.stdin() {
        writer.write_all(content.as_bytes()).await.map_err(|e| {
            kube::Error::Api(kube::error::ErrorResponse {
                status: "500".to_string(),
                message: format!("Failed to write content to stdin: {}", e),
                reason: "WriteError".to_string(),
                code: 500,
            })
        })?;
        drop(writer); // Close stdin to signal end of input
    }

    // Read stdout and stderr to completion to let the process finish
    let mut stdout_output = String::new();
    let mut stderr_output = String::new();

    if let Some(mut stdout) = attach.stdout() {
        stdout.read_to_string(&mut stdout_output).await.ok();
    }
    if let Some(mut stderr) = attach.stderr() {
        stderr.read_to_string(&mut stderr_output).await.ok();
    }

    // Check exit status

    let status = attach.take_status().unwrap().await;
    if let Some(exit_status) = status {
        if let Some(code) = exit_status.code {
            if code != 0 {
                return Err(kube::Error::Api(kube::error::ErrorResponse {
                    status: code.to_string(),
                    message: format!(
                        "write failed: stderr: '{}', stdout: '{}'",
                        stderr_output.trim(),
                        stdout_output.trim()
                    ),
                    reason: "CommandFailed".to_string(),
                    code: code as u16,
                }));
            }
        }
    }

    info!(
        "Successfully wrote spec.json to /var/spec.json in pod {}",
        pod_name
    );
    Ok(())
}
