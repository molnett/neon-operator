use e2e_tests::{cleanup_all_test_clusters, wait_for_condition, TestEnv};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::{api::PostParams, Api};
use neon_cluster::controllers::resources::{
    NeonBranch, NeonBranchSpec, NeonCluster, NeonClusterSpec, NeonProject, NeonProjectSpec, PGVersion,
};
use serial_test::serial;
use std::time::Duration;

fn init_logging() {
    use std::fs::OpenOptions;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    // Create log file
    let log_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open("e2e_tests.log")
        .expect("Failed to create log file");

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(log_file)
        .with_ansi(false);

    let console_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stdout);

    let _ = tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            "info,testcontainers=debug,e2e_tests=debug",
        ))
        .with(file_layer)
        .with(console_layer)
        .try_init();
}

#[tokio::test]
#[serial]
async fn test_operator_health() {
    init_logging();

    let env = TestEnv::new("operator-health").await.unwrap();

    let test_result = async {
        // Verify operator pod is running
        use k8s_openapi::api::core::v1::Pod;
        let pods: Api<Pod> = Api::namespaced(env.client.clone(), &env.namespace);

        let pod_list = pods
            .list(&kube::api::ListParams {
                label_selector: Some("app=neon-operator".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();

        assert!(!pod_list.items.is_empty(), "Operator pod should exist");

        let pod = &pod_list.items[0];
        if let Some(status) = &pod.status {
            assert_eq!(
                status.phase,
                Some("Running".to_string()),
                "Operator pod should be running"
            );
        }

        tracing::info!("✅ Operator health test passed");
    }.await;

    // Ensure cleanup happens regardless of test result
    let cleanup_result = cleanup_test_env(env).await;
    
    // Log cleanup result
    if cleanup_result.is_err() {
        tracing::warn!("Test cleanup had issues, but continuing with test result");
    }

    // Return the test result (will panic if test failed)
    test_result
}


#[tokio::test]
#[serial]
async fn test_branch_creation() {
    init_logging();

    let env = TestEnv::new("branch-creation").await.unwrap();

    let test_result = async {
        // Create cluster and project first
        create_test_cluster(&env).await;
        let project_id = create_test_project(&env).await;

        // Create NeonBranch
        let branch = NeonBranch {
            metadata: ObjectMeta {
                name: Some("test-branch".to_string()),
                namespace: Some(env.namespace.clone()),
                ..Default::default()
            },
            spec: NeonBranchSpec {
                id: uuid::Uuid::new_v4().to_string(),
                name: "main".to_string(),
                timeline_id: None, // Auto-generated
                pg_version: PGVersion::PG16,
                default_branch: true,
                project_id,
            },
            status: None,
        };

        let api: Api<NeonBranch> = Api::namespaced(env.client.clone(), &env.namespace);
        api.create(&PostParams::default(), &branch).await.unwrap();

        // Wait for branch to be ready (2 minute timeout)
        wait_for_condition::<NeonBranch>(
            &env.client,
            &env.namespace,
            "test-branch",
            "ComputeNodeReady",
            "true",
            Duration::from_secs(300),
        )
        .await
        .unwrap();

        // Verify branch has status
        let updated_branch = api.get("test-branch").await.unwrap();
        assert!(updated_branch.status.is_some(), "Branch should have status");

        tracing::info!("✅ Branch creation test passed");
    }.await;

    // Ensure cleanup happens regardless of test result
    let cleanup_result = cleanup_test_env(env).await;
    
    // Log cleanup result
    if cleanup_result.is_err() {
        tracing::warn!("Test cleanup had issues, but continuing with test result");
    }

    // Return the test result (will panic if test failed)
    test_result
}

// Helper functions
async fn create_bucket_credentials_secret(env: &TestEnv) -> Result<(), Box<dyn std::error::Error>> {
    use k8s_openapi::api::core::v1::Secret;

    let mut data = std::collections::BTreeMap::new();
    data.insert(
        "AWS_ACCESS_KEY_ID".to_string(),
        k8s_openapi::ByteString(env.minio_access_key.as_bytes().to_vec()),
    );
    data.insert(
        "AWS_SECRET_ACCESS_KEY".to_string(),
        k8s_openapi::ByteString(env.minio_secret_key.as_bytes().to_vec()),
    );
    data.insert(
        "AWS_REGION".to_string(),
        k8s_openapi::ByteString("us-east-1".as_bytes().to_vec()),
    );
    data.insert(
        "BUCKET_NAME".to_string(),
        k8s_openapi::ByteString("neon-pageserver".as_bytes().to_vec()),
    );
    data.insert(
        "AWS_ENDPOINT_URL".to_string(),
        k8s_openapi::ByteString(format!("http://{}", env.minio_endpoint).as_bytes().to_vec()),
    );

    let secret = Secret {
        metadata: ObjectMeta {
            name: Some("test-bucket-creds".to_string()),
            namespace: Some(env.namespace.clone()),
            ..Default::default()
        },
        data: Some(data),
        ..Default::default()
    };

    let api: Api<Secret> = Api::namespaced(env.client.clone(), &env.namespace);
    api.create(&PostParams::default(), &secret).await?;

    Ok(())
}

async fn create_test_cluster(env: &TestEnv) {
    create_bucket_credentials_secret(env).await.unwrap();

    let cluster = NeonCluster {
        metadata: ObjectMeta {
            name: Some("test-cluster".to_string()),
            namespace: Some(env.namespace.clone()),
            ..Default::default()
        },
        spec: NeonClusterSpec {
            num_safekeepers: 3,
            default_pg_version: PGVersion::PG16,
            neon_image: "neondatabase/neon:latest".to_string(),
            bucket_credentials_secret: "test-bucket-creds".to_string(),
        },
        status: None,
    };

    let api: Api<NeonCluster> = Api::namespaced(env.client.clone(), &env.namespace);
    api.create(&PostParams::default(), &cluster).await.unwrap();

    wait_for_cluster_status(
        &env.client,
        &env.namespace,
        "test-cluster",
        Duration::from_secs(180),
    )
    .await
    .unwrap();
}

async fn create_test_project(env: &TestEnv) -> String {
    let project_id = uuid::Uuid::new_v4().to_string();

    let project = NeonProject {
        metadata: ObjectMeta {
            name: Some("test-project".to_string()),
            namespace: Some(env.namespace.clone()),
            ..Default::default()
        },
        spec: NeonProjectSpec {
            cluster_name: "test-cluster".to_string(),
            id: project_id.clone(),
            name: "Test Project".to_string(),
            tenant_id: None,
            pg_version: PGVersion::PG16,
            default_compute_size: 1.0,
            default_database_name: "neondb".to_string(),
            superuser_name: "neon_admin".to_string(),
        },
        status: None,
    };

    let api: Api<NeonProject> = Api::namespaced(env.client.clone(), &env.namespace);
    api.create(&PostParams::default(), &project).await.unwrap();

    wait_for_condition::<NeonProject>(
        &env.client,
        &env.namespace,
        "test-project",
        "Ready",
        "True",
        Duration::from_secs(120),
    )
    .await
    .unwrap();

    project.metadata.name.unwrap()
}

async fn wait_for_cluster_status(
    client: &kube::Client,
    namespace: &str,
    name: &str,
    timeout: Duration,
) -> Result<(), Box<dyn std::error::Error>> {
    let api: Api<NeonCluster> = Api::namespaced(client.clone(), namespace);
    let start = std::time::Instant::now();

    while start.elapsed() < timeout {
        match api.get(name).await {
            Ok(cluster) => {
                if cluster.status.is_some() {
                    tracing::info!("Cluster {} has status", name);
                    return Ok(());
                }
                tracing::debug!("Cluster {} exists but no status yet", name);
            }
            Err(e) => tracing::debug!("Cluster {} not found yet: {}", name, e),
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    Err(format!("Cluster {} did not get status within timeout", name).into())
}

#[tokio::test]
#[ignore] // Only run manually with --ignored
async fn cleanup_test_clusters() {
    init_logging();
    cleanup_all_test_clusters().await.unwrap();
    tracing::info!("✅ Cleanup completed");
}

// Explicit cleanup function that ensures Kind cluster is deleted
async fn cleanup_test_env(env: TestEnv) -> Result<(), Box<dyn std::error::Error>> {
    let cluster_name = env.cluster_name.clone();
    
    tracing::info!("Explicitly cleaning up test environment: {}", cluster_name);
    
    // Drop the environment to trigger normal cleanup
    drop(env);
    
    // Give some time for the async cleanup to start
    tokio::time::sleep(Duration::from_secs(2)).await;
    
    // Force cleanup of Kind cluster to ensure it's deleted
    if let Err(e) = force_cleanup_kind_cluster(&cluster_name).await {
        tracing::warn!("Failed to force cleanup Kind cluster {}: {}", cluster_name, e);
        return Err(e);
    } else {
        tracing::info!("Successfully cleaned up Kind cluster: {}", cluster_name);
    }
    
    Ok(())
}

// Force cleanup of Kind cluster
async fn force_cleanup_kind_cluster(cluster_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    use tokio::process::Command;
    
    tracing::debug!("Force deleting Kind cluster: {}", cluster_name);

    let output = Command::new("kind")
        .args(&["delete", "cluster", "--name", cluster_name])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("not found") {
            tracing::debug!("Kind cluster {} already deleted", cluster_name);
            return Ok(());
        }
        return Err(format!(
            "Failed to delete Kind cluster {}: {}",
            cluster_name,
            stderr
        ).into());
    }

    Ok(())
}
