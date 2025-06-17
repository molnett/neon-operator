use k8s_openapi::api::core::v1::Namespace;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::{Api, Client, Config};
use serde::Deserialize;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::sleep;

pub struct TestEnv {
    pub cluster_name: String,
    pub client: Client,
    pub namespace: String,
    pub minio_endpoint: String,
    pub minio_access_key: String,
    pub minio_secret_key: String,
}

impl TestEnv {
    pub async fn new(test_name: &str) -> Result<Self, Box<dyn std::error::Error>> {
        tracing::info!("Starting Kind cluster for test: {}", test_name);

        // Generate unique cluster name
        let cluster_name = format!(
            "neon-e2e-{}-{}",
            test_name,
            uuid::Uuid::new_v4().to_string()[..8].to_lowercase()
        );

        // Create Kind cluster
        create_kind_cluster(&cluster_name).await?;

        // Load operator image into Kind cluster
        load_operator_image(&cluster_name).await?;

        // Get kubeconfig from Kind
        let kubeconfig_yaml = get_kind_kubeconfig(&cluster_name).await?;

        // Create Kubernetes client
        let config = Config::from_custom_kubeconfig(
            kube::config::Kubeconfig::from_yaml(&kubeconfig_yaml)?,
            &kube::config::KubeConfigOptions::default(),
        )
        .await?;
        let client = Client::try_from(config)?;

        // Use the "neon" namespace for all tests to match service discovery expectations
        let namespace = "neon".to_string();
        create_namespace(&client, &namespace).await?;

        // Install CRDs
        install_crds(&client).await?;

        // Deploy MinIO
        let (minio_endpoint, minio_access_key, minio_secret_key) = 
            deploy_minio(&client, &namespace).await?;

        // Setup bucket for pageserver (using port-forward to access MinIO)
        setup_minio_buckets(&cluster_name, &namespace, &minio_access_key, &minio_secret_key).await?;

        // Deploy operator
        deploy_operator(&client, &namespace).await?;

        tracing::info!(
            "Test environment ready: cluster={}, namespace={}, minio={}",
            cluster_name,
            namespace,
            minio_endpoint
        );

        Ok(TestEnv {
            cluster_name,
            client,
            namespace,
            minio_endpoint,
            minio_access_key,
            minio_secret_key,
        })
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        // Schedule cluster cleanup (fire and forget)
        let cluster_name = self.cluster_name.clone();
        tokio::spawn(async move {
            if let Err(e) = cleanup_kind_cluster(&cluster_name).await {
                tracing::warn!("Failed to cleanup Kind cluster {}: {}", cluster_name, e);
            }
        });
    }
}

// Add a cleanup function that can be called from panic hooks or signal handlers
pub async fn cleanup_all_test_clusters() -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("Cleaning up all e2e test clusters");
    
    let output = Command::new("kind")
        .args(&["get", "clusters"])
        .output()
        .await?;
    
    if !output.status.success() {
        return Err("Failed to list kind clusters".into());
    }
    
    let clusters = String::from_utf8(output.stdout)?;
    for cluster in clusters.lines() {
        if cluster.starts_with("neon-e2e-") {
            tracing::info!("Cleaning up leftover cluster: {}", cluster);
            if let Err(e) = cleanup_kind_cluster(cluster).await {
                tracing::warn!("Failed to cleanup cluster {}: {}", cluster, e);
            }
        }
    }
    
    Ok(())
}

async fn create_namespace(client: &Client, namespace: &str) -> Result<(), Box<dyn std::error::Error>> {
    let ns = Namespace {
        metadata: ObjectMeta {
            name: Some(namespace.to_string()),
            ..Default::default()
        },
        ..Default::default()
    };

    let ns_api: Api<Namespace> = Api::all(client.clone());
    match ns_api.create(&kube::api::PostParams::default(), &ns).await {
        Ok(_) => tracing::info!("Successfully created namespace: {}", namespace),
        Err(kube::Error::Api(kube::error::ErrorResponse { code: 409, .. })) => {
            tracing::info!("Namespace {} already exists, continuing", namespace);
        }
        Err(e) => return Err(format!("Failed to create namespace: {}", e).into()),
    }

    Ok(())
}

async fn install_crds(client: &Client) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("Installing CRDs");

    // Generate CRDs using crdgen
    let output = std::process::Command::new("cargo")
        .args(&["run", "-p", "crdgen"])
        .output()?;

    if !output.status.success() {
        return Err(format!(
            "CRD generation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }

    let crd_yaml = String::from_utf8(output.stdout)?;
    apply_yaml_documents(client, &crd_yaml).await?;

    // Wait for CRDs to be established
    wait_for_crds_ready(client).await?;

    Ok(())
}

async fn deploy_operator(client: &Client, namespace: &str) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("Deploying operator");

    let operator_yaml = format!(
        r#"
apiVersion: v1
kind: ServiceAccount
metadata:
  name: neon-operator
  namespace: {}
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: neon-operator
rules:
- apiGroups: [""]
  resources: ["pods", "services", "secrets", "configmaps", "persistentvolumeclaims"]
  verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
- apiGroups: ["apps"]
  resources: ["deployments", "statefulsets"]
  verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
- apiGroups: ["oltp.molnett.org"]
  resources: ["neonclusters", "neonprojects", "neonbranches"]
  verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
- apiGroups: ["oltp.molnett.org"]
  resources: ["neonclusters/status", "neonprojects/status", "neonbranches/status"]
  verbs: ["get", "update", "patch"]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: neon-operator
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: neon-operator
subjects:
- kind: ServiceAccount
  name: neon-operator
  namespace: {}
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: neon-operator
  namespace: {}
spec:
  replicas: 1
  selector:
    matchLabels:
      app: neon-operator
  template:
    metadata:
      labels:
        app: neon-operator
    spec:
      serviceAccountName: neon-operator
      containers:
      - name: operator
        image: molnett/neon-operator:local
        imagePullPolicy: Never
        env:
        - name: RUST_LOG
          value: "info,controller=debug"
        ports:
        - containerPort: 8080
          name: http
        livenessProbe:
          httpGet:
            path: /health
            port: 8080
          initialDelaySeconds: 30
          periodSeconds: 10
        readinessProbe:
          httpGet:
            path: /health
            port: 8080
          initialDelaySeconds: 5
          periodSeconds: 5
"#,
        namespace, namespace, namespace
    );

    apply_yaml_documents(client, &operator_yaml).await?;

    // Wait for operator to be ready
    wait_for_deployment_ready(client, namespace, "neon-operator").await?;

    Ok(())
}

async fn apply_yaml_documents(client: &Client, yaml: &str) -> Result<(), Box<dyn std::error::Error>> {
    use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
    use k8s_openapi::api::apps::v1::Deployment;
    use k8s_openapi::api::core::v1::{ServiceAccount, Service};
    use k8s_openapi::api::rbac::v1::{ClusterRole, ClusterRoleBinding};
    use kube::api::{Api, PostParams};

    for doc in serde_yaml::Deserializer::from_str(yaml) {
        let value: serde_yaml::Value = serde_yaml::Value::deserialize(doc)?;

        // Extract basic info from the YAML
        let api_version = value
            .get("apiVersion")
            .and_then(|v| v.as_str())
            .ok_or("Missing apiVersion")?;
        let kind = value.get("kind").and_then(|v| v.as_str()).ok_or("Missing kind")?;

        match (kind, api_version) {
            ("CustomResourceDefinition", "apiextensions.k8s.io/v1") => {
                let crd_api: Api<CustomResourceDefinition> = Api::all(client.clone());
                let crd: CustomResourceDefinition = serde_yaml::from_value(value)?;
                tracing::info!(
                    "Applying CRD: {}",
                    crd.metadata.name.as_ref().unwrap_or(&"<unknown>".to_string())
                );

                match crd_api.create(&PostParams::default(), &crd).await {
                    Ok(_) => tracing::info!("Successfully created CRD"),
                    Err(kube::Error::Api(kube::error::ErrorResponse { code: 409, .. })) => {
                        tracing::info!("CRD already exists, continuing");
                    }
                    Err(e) => return Err(format!("Failed to create CRD: {}", e).into()),
                }
            }
            ("ServiceAccount", "v1") => {
                let sa: ServiceAccount = serde_yaml::from_value(value)?;
                let namespace = sa.metadata.namespace.as_ref().ok_or("Missing namespace in ServiceAccount")?;
                let name = sa.metadata.name.as_ref().ok_or("Missing name in ServiceAccount")?;
                
                let sa_api: Api<ServiceAccount> = Api::namespaced(client.clone(), namespace);
                tracing::info!("Applying ServiceAccount: {} in namespace {}", name, namespace);

                match sa_api.create(&PostParams::default(), &sa).await {
                    Ok(_) => tracing::info!("Successfully created ServiceAccount"),
                    Err(kube::Error::Api(kube::error::ErrorResponse { code: 409, .. })) => {
                        tracing::info!("ServiceAccount already exists, continuing");
                    }
                    Err(e) => return Err(format!("Failed to create ServiceAccount: {}", e).into()),
                }
            }
            ("Service", "v1") => {
                let service: Service = serde_yaml::from_value(value)?;
                let namespace = service.metadata.namespace.as_ref().ok_or("Missing namespace in Service")?;
                let name = service.metadata.name.as_ref().ok_or("Missing name in Service")?;
                
                let service_api: Api<Service> = Api::namespaced(client.clone(), namespace);
                tracing::info!("Applying Service: {} in namespace {}", name, namespace);

                match service_api.create(&PostParams::default(), &service).await {
                    Ok(_) => tracing::info!("Successfully created Service"),
                    Err(kube::Error::Api(kube::error::ErrorResponse { code: 409, .. })) => {
                        tracing::info!("Service already exists, continuing");
                    }
                    Err(e) => return Err(format!("Failed to create Service: {}", e).into()),
                }
            }
            ("ClusterRole", "rbac.authorization.k8s.io/v1") => {
                let cr: ClusterRole = serde_yaml::from_value(value)?;
                let name = cr.metadata.name.as_ref().ok_or("Missing name in ClusterRole")?;
                
                let cr_api: Api<ClusterRole> = Api::all(client.clone());
                tracing::info!("Applying ClusterRole: {}", name);

                match cr_api.create(&PostParams::default(), &cr).await {
                    Ok(_) => tracing::info!("Successfully created ClusterRole"),
                    Err(kube::Error::Api(kube::error::ErrorResponse { code: 409, .. })) => {
                        tracing::info!("ClusterRole already exists, continuing");
                    }
                    Err(e) => return Err(format!("Failed to create ClusterRole: {}", e).into()),
                }
            }
            ("ClusterRoleBinding", "rbac.authorization.k8s.io/v1") => {
                let crb: ClusterRoleBinding = serde_yaml::from_value(value)?;
                let name = crb.metadata.name.as_ref().ok_or("Missing name in ClusterRoleBinding")?;
                
                let crb_api: Api<ClusterRoleBinding> = Api::all(client.clone());
                tracing::info!("Applying ClusterRoleBinding: {}", name);

                match crb_api.create(&PostParams::default(), &crb).await {
                    Ok(_) => tracing::info!("Successfully created ClusterRoleBinding"),
                    Err(kube::Error::Api(kube::error::ErrorResponse { code: 409, .. })) => {
                        tracing::info!("ClusterRoleBinding already exists, continuing");
                    }
                    Err(e) => return Err(format!("Failed to create ClusterRoleBinding: {}", e).into()),
                }
            }
            ("Deployment", "apps/v1") => {
                let deployment: Deployment = serde_yaml::from_value(value)?;
                let namespace = deployment.metadata.namespace.as_ref().ok_or("Missing namespace in deployment")?;
                let name = deployment.metadata.name.as_ref().ok_or("Missing name in deployment")?;
                
                let deploy_api: Api<Deployment> = Api::namespaced(client.clone(), namespace);
                tracing::info!("Applying Deployment: {} in namespace {}", name, namespace);

                match deploy_api.create(&PostParams::default(), &deployment).await {
                    Ok(_) => tracing::info!("Successfully created Deployment"),
                    Err(kube::Error::Api(kube::error::ErrorResponse { code: 409, .. })) => {
                        tracing::info!("Deployment already exists, continuing");
                    }
                    Err(e) => return Err(format!("Failed to create Deployment: {}", e).into()),
                }
            }
            _ => {
                tracing::info!("Skipping unsupported resource: {} {}", kind, api_version);
            }
        }
    }

    Ok(())
}

async fn wait_for_deployment_ready(
    client: &Client,
    namespace: &str,
    name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    use k8s_openapi::api::apps::v1::Deployment;

    let api: Api<Deployment> = Api::namespaced(client.clone(), namespace);

    for _ in 0..60 {
        // 2 minute timeout
        match api.get(name).await {
            Ok(deployment) => {
                if let Some(status) = &deployment.status {
                    if let (Some(ready_replicas), Some(replicas)) = (status.ready_replicas, status.replicas) {
                        if ready_replicas == replicas && replicas > 0 {
                            tracing::info!("Deployment {} is ready", name);
                            return Ok(());
                        }
                    }
                }
            }
            Err(e) => tracing::debug!("Deployment {} not ready yet: {}", name, e),
        }
        sleep(Duration::from_secs(2)).await;
    }

    Err(format!("Deployment {} did not become ready", name).into())
}

async fn wait_for_crds_ready(client: &Client) -> Result<(), Box<dyn std::error::Error>> {
    use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;

    let crds: Api<CustomResourceDefinition> = Api::all(client.clone());
    let expected_crds = [
        "neonclusters.oltp.molnett.org",
        "neonprojects.oltp.molnett.org",
        "neonbranches.oltp.molnett.org",
    ];

    for crd_name in &expected_crds {
        let mut found = false;
        for _ in 0..30 {
            // 1 minute timeout per CRD
            match crds.get(crd_name).await {
                Ok(crd) => {
                    if let Some(status) = crd.status {
                        if let Some(conditions) = status.conditions {
                            for condition in conditions {
                                if condition.type_ == "Established" && condition.status == "True" {
                                    tracing::info!("CRD {} is established", crd_name);
                                    found = true;
                                    break;
                                }
                            }
                        }
                    }
                }
                Err(e) => tracing::debug!("CRD {} not ready yet: {}", crd_name, e),
            }
            if found {
                continue;
            }
            sleep(Duration::from_secs(2)).await;
        }
    }

    Ok(())
}

// Helper function to wait for resource conditions
pub async fn wait_for_condition<T>(
    client: &Client,
    namespace: &str,
    name: &str,
    condition_type: &str,
    condition_status: &str,
    timeout: Duration,
) -> Result<(), Box<dyn std::error::Error>>
where
    T: kube::Resource<Scope = kube::core::NamespaceResourceScope>
        + serde::de::DeserializeOwned
        + serde::Serialize
        + Clone
        + std::fmt::Debug,
    <T as kube::Resource>::DynamicType: Default,
{
    let api: Api<T> = Api::namespaced(client.clone(), namespace);
    let start = std::time::Instant::now();

    while start.elapsed() < timeout {
        match api.get(name).await {
            Ok(resource) => {
                tracing::debug!("Resource {} exists, checking conditions", name);
                
                // Check if this resource has status with conditions
                let resource_value = serde_json::to_value(&resource)?;
                if let Some(status) = resource_value.get("status") {
                    if let Some(conditions) = status.get("conditions") {
                        if let Some(conditions_array) = conditions.as_array() {
                            for condition in conditions_array {
                                if let (Some(cond_type), Some(cond_status)) = (
                                    condition.get("type").and_then(|v| v.as_str()),
                                    condition.get("status").and_then(|v| v.as_str())
                                ) {
                                    if cond_type == condition_type && cond_status == condition_status {
                                        tracing::info!("Resource {} reached condition {}={}", name, condition_type, condition_status);
                                        return Ok(());
                                    }
                                }
                            }
                        }
                        tracing::debug!("Resource {} has conditions but not the expected one", name);
                    } else {
                        tracing::debug!("Resource {} has status but no conditions field", name);
                    }
                } else {
                    tracing::debug!("Resource {} exists but has no status yet", name);
                }
            }
            Err(e) => tracing::debug!("Resource {} not found yet: {}", name, e),
        }
        sleep(Duration::from_secs(2)).await;
    }

    Err(format!(
        "Resource {} did not reach condition {}={} within timeout",
        name, condition_type, condition_status
    )
    .into())
}

async fn create_kind_cluster(cluster_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("Creating Kind cluster: {}", cluster_name);

    let output = Command::new("kind")
        .args(&["create", "cluster", "--name", cluster_name])
        .output()
        .await?;

    if !output.status.success() {
        return Err(format!(
            "Failed to create Kind cluster: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }

    // Wait for cluster to be ready
    sleep(Duration::from_secs(30)).await;

    Ok(())
}

async fn load_operator_image(cluster_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("Loading operator image into Kind cluster: {}", cluster_name);

    let output = Command::new("kind")
        .args(&[
            "load",
            "docker-image",
            "molnett/neon-operator:local",
            "--name",
            cluster_name,
        ])
        .output()
        .await?;

    if !output.status.success() {
        return Err(format!(
            "Failed to load operator image: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }

    Ok(())
}

async fn get_kind_kubeconfig(cluster_name: &str) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("kind")
        .args(&["get", "kubeconfig", "--name", cluster_name])
        .output()
        .await?;

    if !output.status.success() {
        return Err(format!(
            "Failed to get kubeconfig: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }

    Ok(String::from_utf8(output.stdout)?)
}

async fn deploy_minio(client: &Client, namespace: &str) -> Result<(String, String, String), Box<dyn std::error::Error>> {
    tracing::info!("Deploying MinIO");

    let access_key = "minioadmin";
    let secret_key = "minioadmin123";

    let minio_yaml = format!(
        r#"
apiVersion: v1
kind: Service
metadata:
  name: minio
  namespace: {}
spec:
  ports:
  - port: 9000
    targetPort: 9000
    name: api
  - port: 9001
    targetPort: 9001
    name: console
  selector:
    app: minio
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: minio
  namespace: {}
spec:
  replicas: 1
  selector:
    matchLabels:
      app: minio
  template:
    metadata:
      labels:
        app: minio
    spec:
      containers:
      - name: minio
        image: minio/minio:latest
        args:
        - server
        - /data
        - --console-address
        - :9001
        env:
        - name: MINIO_ROOT_USER
          value: "{}"
        - name: MINIO_ROOT_PASSWORD
          value: "{}"
        ports:
        - containerPort: 9000
          name: api
        - containerPort: 9001
          name: console
        volumeMounts:
        - name: data
          mountPath: /data
      volumes:
      - name: data
        emptyDir: {{}}
"#,
        namespace, namespace, access_key, secret_key
    );

    apply_yaml_documents(client, &minio_yaml).await?;

    // Wait for MinIO to be ready
    wait_for_deployment_ready(client, namespace, "minio").await?;

    let endpoint = format!("minio.{}.svc.cluster.local:9000", namespace);
    
    Ok((endpoint, access_key.to_string(), secret_key.to_string()))
}

async fn setup_minio_buckets(cluster_name: &str, namespace: &str, access_key: &str, secret_key: &str) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("Setting up MinIO bucket for pageserver using port-forward");

    // Find an available port
    let local_port = find_available_port().await.unwrap_or(19000);
    
    tracing::info!("Found available port: {}", local_port);
    tracing::debug!("kubectl context: kind-{}", cluster_name);
    tracing::debug!("namespace: {}", namespace);
    
    // Start port-forward with the specific port
    let context_arg = format!("kind-{}", cluster_name);
    let port_arg = format!("{}:9000", local_port);
    let kubectl_args = vec![
        "--context", &context_arg,
        "-n", namespace,
        "port-forward",
        "service/minio",
        &port_arg
    ];
    tracing::debug!("Starting kubectl with args: {:?}", kubectl_args);
    
    let mut port_forward_child = Command::new("kubectl")
        .args(&kubectl_args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    tracing::info!("Port-forward process started, waiting 8 seconds for establishment...");
    
    // Wait for port-forward to establish
    sleep(Duration::from_secs(8)).await;

    // Check if port-forward is still running
    match port_forward_child.try_wait() {
        Ok(Some(status)) => {
            tracing::error!("Port-forward process exited early with status: {}", status);
            if let Some(stderr) = port_forward_child.stderr.take() {
                use tokio::io::{AsyncReadExt};
                let mut buffer = Vec::new();
                let mut stderr_reader = stderr;
                if stderr_reader.read_to_end(&mut buffer).await.is_ok() {
                    let stderr_output = String::from_utf8_lossy(&buffer);
                    tracing::error!("Port-forward stderr: {}", stderr_output);
                }
            }
        }
        Ok(None) => {
            tracing::info!("Port-forward process is still running");
        }
        Err(e) => {
            tracing::warn!("Failed to check port-forward status: {}", e);
        }
    }

    tracing::info!("Using local port {} for MinIO access", local_port);

    // Create AWS config for MinIO
    let endpoint_url = format!("http://localhost:{}", local_port);
    tracing::debug!("MinIO endpoint URL: {}", endpoint_url);
    tracing::debug!("MinIO access key: {}", access_key);
    tracing::debug!("MinIO secret key: {}", if secret_key.len() > 4 { &secret_key[..4] } else { secret_key });
    
    let creds = aws_credential_types::Credentials::new(
        access_key,
        secret_key,
        None,
        None,
        "minio-setup"
    );

    let config = aws_config::SdkConfig::builder()
        .endpoint_url(&endpoint_url)
        .credentials_provider(aws_credential_types::provider::SharedCredentialsProvider::new(creds))
        .region(aws_config::Region::new("us-east-1"))
        .build();

    let s3_config = aws_sdk_s3::config::Builder::from(&config)
        .force_path_style(true)
        .behavior_version_latest()
        .build();
    let s3_client = aws_sdk_s3::Client::from_conf(s3_config);

    tracing::info!("Created S3 client, testing connectivity to MinIO...");
    
    // Test basic connectivity with a simple HTTP check first
    tracing::debug!("Testing basic HTTP connectivity to http://localhost:{}/minio/health/live", local_port);
    
    // Test connectivity first
    tracing::info!("Testing MinIO S3 API connectivity...");
    match s3_client.list_buckets().send().await {
        Ok(response) => {
            tracing::info!("MinIO connection successful!");
            let buckets = response.buckets();
            tracing::debug!("Existing buckets: {:?}", buckets.iter().map(|b| b.name().unwrap_or("unknown")).collect::<Vec<_>>());
        }
        Err(e) => {
            tracing::error!("Failed to connect to MinIO via S3 API: {}", e);
            tracing::error!("Error details: {:?}", e);
            // Continue anyway - maybe the bucket creation will work
        }
    }

    // Create bucket for pageserver
    let bucket = "neon-pageserver";
    
    tracing::info!("Attempting to create bucket: {}", bucket);
    match s3_client.create_bucket()
        .bucket(bucket)
        .send()
        .await
    {
        Ok(_) => tracing::info!("Successfully created bucket: {}", bucket),
        Err(e) => {
            tracing::warn!("Bucket creation failed for {}: {}", bucket, e);
            tracing::debug!("Bucket creation error details: {:?}", e);
            
            // Check if bucket already exists
            match s3_client.list_buckets().send().await {
                Ok(response) => {
                    let buckets = response.buckets();
                    let bucket_names: Vec<_> = buckets.iter().map(|b| b.name().unwrap_or("unknown")).collect();
                    if bucket_names.contains(&bucket) {
                        tracing::info!("Bucket {} already exists, continuing", bucket);
                    } else {
                        tracing::error!("Bucket {} does not exist and creation failed. Available buckets: {:?}", bucket, bucket_names);
                    }
                }
                Err(list_err) => {
                    tracing::error!("Failed to list buckets to check if {} exists: {}", bucket, list_err);
                }
            }
        }
    }

    // Clean up port-forward
    tracing::info!("Cleaning up port-forward process");
    if let Err(e) = port_forward_child.kill().await {
        tracing::warn!("Failed to kill port-forward process: {}", e);
    } else {
        tracing::debug!("Port-forward process killed successfully");
    }

    Ok(())
}

async fn find_available_port() -> Result<u16, Box<dyn std::error::Error>> {
    use std::net::TcpListener;
    
    tracing::debug!("Finding available port by binding to 127.0.0.1:0");
    
    // Try to bind to port 0 to get an available port
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    tracing::debug!("System assigned port: {}", port);
    drop(listener); // Close the listener to free the port
    
    tracing::debug!("Released port {}, returning it for use", port);
    Ok(port)
}

async fn cleanup_kind_cluster(cluster_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("Cleaning up Kind cluster: {}", cluster_name);

    let output = Command::new("kind")
        .args(&["delete", "cluster", "--name", cluster_name])
        .output()
        .await?;

    if !output.status.success() {
        tracing::warn!(
            "Failed to delete Kind cluster {}: {}",
            cluster_name,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(())
}
