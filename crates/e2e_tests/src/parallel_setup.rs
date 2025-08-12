use crate::state_machine::{TestState, TestStateMachine};
use crate::waiting::{with_progress_indicator, BackoffConfig, SmartWaiter};
use kube::{Api, Client, Config};
use std::time::Duration;
use tokio::process::Command;
use tracing::{debug, info, warn};
use uuid::Uuid;

pub struct ParallelSetupOrchestrator {
    test_name: String,
    cluster_name: String,
}

impl ParallelSetupOrchestrator {
    pub fn new(test_name: &str) -> Self {
        let cluster_name = format!(
            "neon-e2e-{}-{}",
            test_name,
            Uuid::new_v4().to_string()[..8].to_lowercase()
        );

        Self {
            test_name: test_name.to_string(),
            cluster_name,
        }
    }

    pub async fn setup_infrastructure(
        &self,
        state_machine: &mut TestStateMachine,
    ) -> Result<InfrastructureResult, String> {
        info!(
            test_name = self.test_name,
            cluster_name = self.cluster_name,
            "🚀 Starting parallel infrastructure setup"
        );

        state_machine.transition_to(TestState::ClusterCreating)?;

        // Phase 1: Parallel cluster creation and CRD generation
        let (_cluster_result, crd_yaml) = self.phase1_cluster_and_crds().await?;

        state_machine.transition_to(TestState::ImagesLoading)?;

        // Phase 2: Parallel image loading and CRD installation
        let (client, _) = self.phase2_images_and_crds(crd_yaml).await?;

        state_machine.transition_to(TestState::InfrastructureReady)?;

        Ok(InfrastructureResult {
            cluster_name: self.cluster_name.clone(),
            client,
            namespace: "neon".to_string(),
        })
    }

    pub async fn setup_infrastructure_sequential(
        &self,
        state_machine: &mut TestStateMachine,
    ) -> Result<InfrastructureResult, String> {
        info!(
            test_name = self.test_name,
            cluster_name = self.cluster_name,
            "🚀 Starting sequential infrastructure setup (debug mode)"
        );

        state_machine.transition_to(TestState::ClusterCreating)?;

        // Sequential cluster creation
        let _ = self.create_kind_cluster().await?;

        state_machine.transition_to(TestState::ImagesLoading)?;

        // Sequential CRD generation and image loading
        let crd_yaml = self.generate_crds().await?;
        let client = self.create_kubernetes_client().await?;
        let _ = self.load_operator_image().await?;

        state_machine.transition_to(TestState::CRDsInstalling)?;

        // Install CRDs
        let _ = self.install_crds(&client, crd_yaml).await?;

        state_machine.transition_to(TestState::InfrastructureReady)?;

        Ok(InfrastructureResult {
            cluster_name: self.cluster_name.clone(),
            client,
            namespace: "neon".to_string(),
        })
    }

    pub async fn deploy_services(
        &self,
        client: &Client,
        namespace: &str,
        state_machine: &mut TestStateMachine,
    ) -> Result<ServiceResult, String> {
        info!(
            test_name = self.test_name,
            namespace = namespace,
            "🚀 Starting parallel service deployment"
        );

        state_machine.transition_to(TestState::MinioDeploying)?;

        // Phase 3: Parallel MinIO and operator deployment preparation
        let (minio_result, operator_yaml) = self.phase3_services_preparation(client, namespace).await?;

        state_machine.transition_to(TestState::OperatorDeploying)?;

        // Phase 4: Deploy operator and wait for readiness
        let operator_result = self
            .phase4_operator_deployment(client, namespace, operator_yaml)
            .await?;

        state_machine.transition_to(TestState::ComponentsReady)?;

        Ok(ServiceResult {
            minio_endpoint: minio_result.endpoint,
            minio_access_key: minio_result.access_key,
            minio_secret_key: minio_result.secret_key,
            operator_ready: operator_result,
        })
    }

    pub async fn deploy_services_sequential(
        &self,
        client: &Client,
        namespace: &str,
        state_machine: &mut TestStateMachine,
    ) -> Result<ServiceResult, String> {
        info!(
            test_name = self.test_name,
            namespace = namespace,
            "🚀 Starting sequential service deployment (debug mode)"
        );

        state_machine.transition_to(TestState::MinioDeploying)?;

        // Sequential deployment: namespace first
        let _ = self.create_namespace(client, namespace).await?;

        // Then MinIO
        let minio_result = self.deploy_minio(client, namespace).await?;

        state_machine.transition_to(TestState::OperatorDeploying)?;

        // Then operator
        let operator_yaml = self.generate_operator_yaml(namespace).await?;
        let operator_result = self.deploy_operator(client, namespace, operator_yaml).await?;

        state_machine.transition_to(TestState::ComponentsReady)?;

        Ok(ServiceResult {
            minio_endpoint: minio_result.endpoint,
            minio_access_key: minio_result.access_key,
            minio_secret_key: minio_result.secret_key,
            operator_ready: operator_result,
        })
    }

    async fn phase1_cluster_and_crds(&self) -> Result<((), String), String> {
        info!("📦 Phase 1: Creating cluster and generating CRDs in parallel");

        let cluster_future = with_progress_indicator(
            "kind_cluster_creation",
            Duration::from_secs(45),
            self.create_kind_cluster(),
        );

        let crd_future =
            with_progress_indicator("crd_generation", Duration::from_secs(10), self.generate_crds());

        let (cluster_result, crd_result) = tokio::try_join!(cluster_future, crd_future)?;

        info!("✅ Phase 1 complete: Cluster and CRDs ready");
        Ok((cluster_result, crd_result))
    }

    async fn phase2_images_and_crds(&self, crd_yaml: String) -> Result<(Client, ()), String> {
        info!("📦 Phase 2: Loading images and installing CRDs in parallel");

        let client_future = with_progress_indicator(
            "kubernetes_client_setup",
            Duration::from_secs(10),
            self.create_kubernetes_client(),
        );

        let image_future = with_progress_indicator(
            "operator_image_loading",
            Duration::from_secs(15),
            self.load_operator_image(),
        );

        let (client, _) = tokio::try_join!(client_future, image_future)?;

        // Install CRDs after client is ready
        let crd_result = with_progress_indicator(
            "crd_installation",
            Duration::from_secs(20),
            self.install_crds(&client, crd_yaml),
        )
        .await?;

        info!("✅ Phase 2 complete: Images loaded and CRDs installed");
        Ok((client, crd_result))
    }

    async fn phase3_services_preparation(
        &self,
        client: &Client,
        namespace: &str,
    ) -> Result<(MinioResult, String), String> {
        info!("📦 Phase 3: Preparing MinIO and operator deployment in parallel");

        let namespace_future = with_progress_indicator(
            "namespace_creation",
            Duration::from_secs(5),
            self.create_namespace(client, namespace),
        );

        let minio_future = with_progress_indicator(
            "minio_deployment",
            Duration::from_secs(30),
            self.deploy_minio(client, namespace),
        );

        let operator_yaml_future = with_progress_indicator(
            "operator_yaml_generation",
            Duration::from_secs(2),
            self.generate_operator_yaml(namespace),
        );

        let (_, minio_result, operator_yaml) =
            tokio::try_join!(namespace_future, minio_future, operator_yaml_future)?;

        info!("✅ Phase 3 complete: MinIO deployed and operator YAML ready");
        Ok((minio_result, operator_yaml))
    }

    async fn phase4_operator_deployment(
        &self,
        client: &Client,
        namespace: &str,
        operator_yaml: String,
    ) -> Result<bool, String> {
        info!("📦 Phase 4: Deploying operator");

        with_progress_indicator(
            "operator_deployment",
            Duration::from_secs(20),
            self.deploy_operator(client, namespace, operator_yaml),
        )
        .await
    }

    async fn create_kind_cluster(&self) -> Result<(), String> {
        info!(cluster_name = self.cluster_name, "Creating Kind cluster");

        let output = Command::new("kind")
            .args(&["create", "cluster", "--name", &self.cluster_name])
            .output()
            .await
            .map_err(|e| format!("Failed to execute kind command: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "Failed to create Kind cluster: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        // Wait for cluster to be ready with smart waiting
        let waiter = SmartWaiter::with_config("kind_cluster_ready", BackoffConfig::default());
        waiter
            .wait_for(|| async {
                let status = Command::new("kubectl")
                    .args(&[
                        "--context",
                        &format!("kind-{}", self.cluster_name),
                        "get",
                        "nodes",
                    ])
                    .output()
                    .await;

                match status {
                    Ok(output) if output.status.success() => {
                        debug!("Kind cluster is responding to kubectl");
                        Ok(Some(()))
                    }
                    _ => {
                        debug!("Kind cluster not ready yet");
                        Ok(None)
                    }
                }
            })
            .await?;

        info!(cluster_name = self.cluster_name, "✅ Kind cluster ready");
        Ok(())
    }

    async fn generate_crds(&self) -> Result<String, String> {
        info!("Generating CRDs using crdgen");

        let output = tokio::process::Command::new("cargo")
            .args(&["run", "-p", "crdgen"])
            .output()
            .await
            .map_err(|e| format!("Failed to execute crdgen: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "CRD generation failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let crd_yaml =
            String::from_utf8(output.stdout).map_err(|e| format!("Invalid UTF-8 in CRD output: {}", e))?;

        info!(crd_count = crd_yaml.split("---").count(), "✅ CRDs generated");
        Ok(crd_yaml)
    }

    async fn create_kubernetes_client(&self) -> Result<Client, String> {
        info!("Creating Kubernetes client");

        let kubeconfig_yaml = self.get_kind_kubeconfig().await?;

        let config = Config::from_custom_kubeconfig(
            kube::config::Kubeconfig::from_yaml(&kubeconfig_yaml)
                .map_err(|e| format!("Failed to parse kubeconfig: {}", e))?,
            &kube::config::KubeConfigOptions::default(),
        )
        .await
        .map_err(|e| format!("Failed to create client config: {}", e))?;

        let client =
            Client::try_from(config).map_err(|e| format!("Failed to create Kubernetes client: {}", e))?;

        info!("✅ Kubernetes client ready");
        Ok(client)
    }

    async fn load_operator_image(&self) -> Result<(), String> {
        info!(
            cluster_name = self.cluster_name,
            "Loading operator image into Kind cluster"
        );

        let output = Command::new("kind")
            .args(&[
                "load",
                "docker-image",
                "molnett/neon-operator:local",
                "--name",
                &self.cluster_name,
            ])
            .output()
            .await
            .map_err(|e| format!("Failed to execute kind load: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "Failed to load operator image: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        info!("✅ Operator image loaded");
        Ok(())
    }

    async fn install_crds(&self, client: &Client, crd_yaml: String) -> Result<(), String> {
        info!("Installing CRDs into cluster");

        // Apply CRDs
        self.apply_yaml_documents(client, &crd_yaml).await?;

        // Wait for all CRDs to be established in parallel
        let expected_crds = [
            "neonclusters.oltp.molnett.org",
            "neonprojects.oltp.molnett.org",
            "neonbranches.oltp.molnett.org",
            "neonpageservers.oltp.molnett.org",
        ];

        let waiter = SmartWaiter::with_config("crd_establishment", BackoffConfig::fast());

        let crd_futures = expected_crds
            .iter()
            .map(|crd_name| waiter.wait_for_crd_established(client, crd_name));

        // Wait for all CRDs to be established
        futures::future::try_join_all(crd_futures).await?;

        info!(crd_count = expected_crds.len(), "✅ All CRDs established");
        Ok(())
    }

    async fn create_namespace(&self, client: &Client, namespace: &str) -> Result<(), String> {
        use k8s_openapi::api::core::v1::Namespace;
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

        info!(namespace = namespace, "Creating namespace");

        let ns = Namespace {
            metadata: ObjectMeta {
                name: Some(namespace.to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        let ns_api: Api<Namespace> = Api::all(client.clone());
        match ns_api.create(&kube::api::PostParams::default(), &ns).await {
            Ok(_) => info!(namespace = namespace, "✅ Namespace created"),
            Err(kube::Error::Api(kube::error::ErrorResponse { code: 409, .. })) => {
                info!(namespace = namespace, "Namespace already exists");
            }
            Err(e) => return Err(format!("Failed to create namespace: {}", e)),
        }

        Ok(())
    }

    async fn deploy_minio(&self, client: &Client, namespace: &str) -> Result<MinioResult, String> {
        info!(namespace = namespace, "Deploying MinIO");

        let access_key = "minioadmin";
        let secret_key = "minioadmin123";

        let minio_yaml = self.generate_minio_yaml(namespace, access_key, secret_key);
        self.apply_yaml_documents(client, &minio_yaml).await?;

        // Wait for MinIO to be ready
        let waiter = SmartWaiter::with_config("minio_deployment", BackoffConfig::default());
        waiter
            .wait_for_deployment_ready(client, namespace, "minio")
            .await?;

        // Setup bucket
        self.setup_minio_bucket(namespace, access_key, secret_key).await?;

        let endpoint = format!("minio.{}.svc.cluster.local:9000", namespace);

        info!(endpoint = endpoint, "✅ MinIO deployed and ready");
        Ok(MinioResult {
            endpoint,
            access_key: access_key.to_string(),
            secret_key: secret_key.to_string(),
        })
    }

    async fn generate_operator_yaml(&self, namespace: &str) -> Result<String, String> {
        info!("Generating operator deployment YAML");

        let operator_yaml = format!(
            r#"apiVersion: v1
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
- apiGroups:
  - ""
  resources:
  - pods/exec
  verbs:
  - create
  - get
- apiGroups:
  - "events.k8s.io"
  resources:
  - events
  verbs:
  - create
- apiGroups:
  - ""
  resources:
  - pods
  - services
  - services/finalizers
  - endpoints
  - persistentvolumeclaims
  - events
  - configmaps
  - secrets
  verbs:
  - create
  - delete
  - get
  - list
  - patch
  - update
  - watch
- apiGroups:
  - apps
  resources:
  - deployments
  - daemonsets
  - replicasets
  - statefulsets
  verbs:
  - create
  - delete
  - get
  - list
  - patch
  - update
  - watch
- apiGroups:
  - oltp.molnett.org
  resources:
  - neonclusters
  - neonclusters/status
  - neonclusters/finalizers
  - neonpageservers
  - neonpageservers/status
  - neonpageservers/finalizers
  - neonprojects
  - neonprojects/status
  - neonprojects/finalizers
  - neonbranches
  - neonbranches/status
  - neonbranches/finalizers
  verbs:
  - create
  - delete
  - get
  - list
  - patch
  - update
  - watch
- apiGroups:
  - coordination.k8s.io
  resources:
  - leases
  verbs:
  - create
  - get
  - list
  - update
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
    labels:
        app: neon-operator
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
            securityContext:
                runAsNonRoot: true
                seccompProfile:
                    type: RuntimeDefault
            containers:
            - name: operator
              image: molnett/neon-operator:local
              imagePullPolicy: Never
              command:
              - /app/operator
              securityContext:
                  allowPrivilegeEscalation: false
                  capabilities:
                      drop:
                      - ALL
              resources:
                  limits:
                      cpu: 500m
                      memory: 512Mi
                  requests:
                      cpu: 100m
                      memory: 128Mi
---
apiVersion: v1
kind: Service
metadata:
    name: neon-operator
    namespace: {}
spec:
    selector:
        app: neon-operator
    ports:
    - name: http
      port: 8080
      targetPort: 8080
"#,
            namespace, namespace, namespace, namespace
        );

        info!("✅ Operator YAML generated");
        Ok(operator_yaml)
    }

    async fn deploy_operator(
        &self,
        client: &Client,
        namespace: &str,
        operator_yaml: String,
    ) -> Result<bool, String> {
        info!(namespace = namespace, "Deploying operator");

        self.apply_yaml_documents(client, &operator_yaml).await?;

        // Wait for operator to be ready
        let waiter = SmartWaiter::with_config("operator_deployment", BackoffConfig::default());
        waiter
            .wait_for_deployment_ready(client, namespace, "neon-operator")
            .await?;

        info!("✅ Operator deployed and ready");
        Ok(true)
    }

    async fn get_kind_kubeconfig(&self) -> Result<String, String> {
        let output = Command::new("kind")
            .args(&["get", "kubeconfig", "--name", &self.cluster_name])
            .output()
            .await
            .map_err(|e| format!("Failed to get kubeconfig: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "Failed to get kubeconfig: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(String::from_utf8(output.stdout).map_err(|e| format!("Invalid UTF-8 in kubeconfig: {}", e))?)
    }

    fn generate_minio_yaml(&self, namespace: &str, access_key: &str, secret_key: &str) -> String {
        format!(
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
        )
    }

    async fn setup_minio_bucket(
        &self,
        namespace: &str,
        access_key: &str,
        secret_key: &str,
    ) -> Result<(), String> {
        // Use port-forward to setup bucket with connection verification
        let local_port = self.find_available_port().await?;

        let context_arg = format!("kind-{}", self.cluster_name);
        let port_arg = format!("{}:9000", local_port);

        let mut port_forward_child = Command::new("kubectl")
            .args(&[
                "--context",
                &context_arg,
                "-n",
                namespace,
                "port-forward",
                "service/minio",
                &port_arg,
            ])
            .spawn()
            .map_err(|e| format!("Failed to start port-forward: {}", e))?;

        // Wait for port-forward to be ready with actual connection verification
        let endpoint_url = format!("http://localhost:{}", local_port);
        let waiter = SmartWaiter::with_config("minio_connection", BackoffConfig::fast());

        let connection_result = waiter
            .wait_for_http_endpoint(&format!("{}/minio/health/live", endpoint_url), Some(200))
            .await;

        if connection_result.is_err() {
            let _ = port_forward_child.kill().await;
            return Err("Failed to establish MinIO connection".to_string());
        }

        // Setup S3 client and create bucket
        let bucket_result = self
            .create_minio_bucket(&endpoint_url, access_key, secret_key)
            .await;

        // Cleanup port-forward
        if let Err(e) = port_forward_child.kill().await {
            warn!("Failed to kill port-forward process: {}", e);
        }

        bucket_result
    }

    async fn create_minio_bucket(
        &self,
        endpoint_url: &str,
        access_key: &str,
        secret_key: &str,
    ) -> Result<(), String> {
        let creds = aws_credential_types::Credentials::new(access_key, secret_key, None, None, "minio-setup");

        let config = aws_config::SdkConfig::builder()
            .endpoint_url(endpoint_url)
            .credentials_provider(aws_credential_types::provider::SharedCredentialsProvider::new(
                creds,
            ))
            .region(aws_config::Region::new("us-east-1"))
            .build();

        let s3_config = aws_sdk_s3::config::Builder::from(&config)
            .force_path_style(true)
            .behavior_version_latest()
            .build();

        let s3_client = aws_sdk_s3::Client::from_conf(s3_config);

        // Create bucket
        let bucket_name = "neon-pageserver";
        match s3_client.create_bucket().bucket(bucket_name).send().await {
            Ok(_) => {
                info!(bucket = bucket_name, "✅ MinIO bucket created");
                Ok(())
            }
            Err(e) => {
                // Check if bucket already exists
                match s3_client.list_buckets().send().await {
                    Ok(response) => {
                        let bucket_names: Vec<_> = response
                            .buckets()
                            .iter()
                            .map(|b| b.name().unwrap_or("unknown"))
                            .collect();

                        if bucket_names.contains(&bucket_name) {
                            info!(bucket = bucket_name, "MinIO bucket already exists");
                            Ok(())
                        } else {
                            Err(format!("Failed to create bucket and it doesn't exist: {}", e))
                        }
                    }
                    Err(list_err) => Err(format!(
                        "Failed to create bucket and couldn't verify: {}",
                        list_err
                    )),
                }
            }
        }
    }

    async fn find_available_port(&self) -> Result<u16, String> {
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| format!("Failed to bind to available port: {}", e))?;

        let port = listener
            .local_addr()
            .map_err(|e| format!("Failed to get local address: {}", e))?
            .port();

        drop(listener);
        Ok(port)
    }

    async fn apply_yaml_documents(&self, client: &Client, yaml: &str) -> Result<(), String> {
        use k8s_openapi::api::apps::v1::Deployment;
        use k8s_openapi::api::core::v1::{Service, ServiceAccount};
        use k8s_openapi::api::rbac::v1::{ClusterRole, ClusterRoleBinding};
        use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
        use kube::api::{Api, PostParams};
        use serde::Deserialize;

        for doc in serde_yaml::Deserializer::from_str(yaml) {
            let value: serde_yaml::Value = serde_yaml::Value::deserialize(doc)
                .map_err(|e| format!("Failed to deserialize YAML: {}", e))?;

            let api_version = value
                .get("apiVersion")
                .and_then(|v| v.as_str())
                .ok_or("Missing apiVersion")?;
            let kind = value.get("kind").and_then(|v| v.as_str()).ok_or("Missing kind")?;

            match (kind, api_version) {
                ("CustomResourceDefinition", "apiextensions.k8s.io/v1") => {
                    let crd_api: Api<CustomResourceDefinition> = Api::all(client.clone());
                    let crd: CustomResourceDefinition =
                        serde_yaml::from_value(value).map_err(|e| format!("Failed to parse CRD: {}", e))?;

                    let name = crd.metadata.name.as_ref().unwrap();
                    debug!(crd = name, "Applying CRD");

                    match crd_api.create(&PostParams::default(), &crd).await {
                        Ok(_) => debug!(crd = name, "CRD created"),
                        Err(kube::Error::Api(kube::error::ErrorResponse { code: 409, .. })) => {
                            debug!(crd = name, "CRD already exists");
                        }
                        Err(e) => return Err(format!("Failed to create CRD {}: {}", name, e)),
                    }
                }
                ("ServiceAccount", "v1") => {
                    let sa: ServiceAccount = serde_yaml::from_value(value)
                        .map_err(|e| format!("Failed to parse ServiceAccount: {}", e))?;
                    let namespace = sa
                        .metadata
                        .namespace
                        .as_ref()
                        .ok_or("Missing namespace in ServiceAccount")?;
                    let name = sa
                        .metadata
                        .name
                        .as_ref()
                        .ok_or("Missing name in ServiceAccount")?;

                    let sa_api: Api<ServiceAccount> = Api::namespaced(client.clone(), namespace);
                    debug!(
                        service_account = name,
                        namespace = namespace,
                        "Applying ServiceAccount"
                    );

                    match sa_api.create(&PostParams::default(), &sa).await {
                        Ok(_) => debug!(service_account = name, "ServiceAccount created"),
                        Err(kube::Error::Api(kube::error::ErrorResponse { code: 409, .. })) => {
                            debug!(service_account = name, "ServiceAccount already exists");
                        }
                        Err(e) => return Err(format!("Failed to create ServiceAccount {}: {}", name, e)),
                    }
                }
                ("Service", "v1") => {
                    let service: Service = serde_yaml::from_value(value)
                        .map_err(|e| format!("Failed to parse Service: {}", e))?;
                    let namespace = service
                        .metadata
                        .namespace
                        .as_ref()
                        .ok_or("Missing namespace in Service")?;
                    let name = service.metadata.name.as_ref().ok_or("Missing name in Service")?;

                    let service_api: Api<Service> = Api::namespaced(client.clone(), namespace);
                    debug!(service = name, namespace = namespace, "Applying Service");

                    match service_api.create(&PostParams::default(), &service).await {
                        Ok(_) => debug!(service = name, "Service created"),
                        Err(kube::Error::Api(kube::error::ErrorResponse { code: 409, .. })) => {
                            debug!(service = name, "Service already exists");
                        }
                        Err(e) => return Err(format!("Failed to create Service {}: {}", name, e)),
                    }
                }
                ("ClusterRole", "rbac.authorization.k8s.io/v1") => {
                    let cr: ClusterRole = serde_yaml::from_value(value)
                        .map_err(|e| format!("Failed to parse ClusterRole: {}", e))?;
                    let name = cr.metadata.name.as_ref().ok_or("Missing name in ClusterRole")?;

                    let cr_api: Api<ClusterRole> = Api::all(client.clone());
                    debug!(cluster_role = name, "Applying ClusterRole");

                    match cr_api.create(&PostParams::default(), &cr).await {
                        Ok(_) => debug!(cluster_role = name, "ClusterRole created"),
                        Err(kube::Error::Api(kube::error::ErrorResponse { code: 409, .. })) => {
                            debug!(cluster_role = name, "ClusterRole already exists");
                        }
                        Err(e) => return Err(format!("Failed to create ClusterRole {}: {}", name, e)),
                    }
                }
                ("ClusterRoleBinding", "rbac.authorization.k8s.io/v1") => {
                    let crb: ClusterRoleBinding = serde_yaml::from_value(value)
                        .map_err(|e| format!("Failed to parse ClusterRoleBinding: {}", e))?;
                    let name = crb
                        .metadata
                        .name
                        .as_ref()
                        .ok_or("Missing name in ClusterRoleBinding")?;

                    let crb_api: Api<ClusterRoleBinding> = Api::all(client.clone());
                    debug!(cluster_role_binding = name, "Applying ClusterRoleBinding");

                    match crb_api.create(&PostParams::default(), &crb).await {
                        Ok(_) => debug!(cluster_role_binding = name, "ClusterRoleBinding created"),
                        Err(kube::Error::Api(kube::error::ErrorResponse { code: 409, .. })) => {
                            debug!(cluster_role_binding = name, "ClusterRoleBinding already exists");
                        }
                        Err(e) => return Err(format!("Failed to create ClusterRoleBinding {}: {}", name, e)),
                    }
                }
                ("Deployment", "apps/v1") => {
                    let deployment: Deployment = serde_yaml::from_value(value)
                        .map_err(|e| format!("Failed to parse Deployment: {}", e))?;
                    let namespace = deployment
                        .metadata
                        .namespace
                        .as_ref()
                        .ok_or("Missing namespace in deployment")?;
                    let name = deployment
                        .metadata
                        .name
                        .as_ref()
                        .ok_or("Missing name in deployment")?;

                    let deploy_api: Api<Deployment> = Api::namespaced(client.clone(), namespace);
                    debug!(deployment = name, namespace = namespace, "Applying Deployment");

                    match deploy_api.create(&PostParams::default(), &deployment).await {
                        Ok(_) => debug!(deployment = name, "Deployment created"),
                        Err(kube::Error::Api(kube::error::ErrorResponse { code: 409, .. })) => {
                            debug!(deployment = name, "Deployment already exists");
                        }
                        Err(e) => return Err(format!("Failed to create Deployment {}: {}", name, e)),
                    }
                }
                _ => {
                    debug!(
                        kind = kind,
                        api_version = api_version,
                        "Skipping unsupported resource"
                    );
                }
            }
        }

        Ok(())
    }

    /// Clean up infrastructure for a specific cluster
    pub async fn cleanup_infrastructure(&self, cluster_name: &str) -> Result<(), String> {
        info!(cluster_name = cluster_name, "Cleaning up infrastructure");

        let output = Command::new("kind")
            .args(&["delete", "cluster", "--name", cluster_name])
            .output()
            .await
            .map_err(|e| format!("Failed to execute kind delete: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("not found") {
                info!(cluster_name = cluster_name, "Cluster already deleted");
                return Ok(());
            }
            return Err(format!("Failed to delete cluster {}: {}", cluster_name, stderr));
        }

        info!(cluster_name = cluster_name, "✅ Infrastructure cleaned up");
        Ok(())
    }

    /// Clean up all test clusters matching the neon-e2e pattern
    pub async fn cleanup_all_test_clusters(&self) -> Result<(), String> {
        info!("Cleaning up all test clusters");

        // Get list of all Kind clusters
        let output = Command::new("kind")
            .args(&["get", "clusters"])
            .output()
            .await
            .map_err(|e| format!("Failed to list Kind clusters: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "Failed to list clusters: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let clusters_output = String::from_utf8(output.stdout)
            .map_err(|e| format!("Invalid UTF-8 in clusters output: {}", e))?;

        let test_clusters: Vec<&str> = clusters_output
            .lines()
            .filter(|line| line.starts_with("neon-e2e-"))
            .collect();

        if test_clusters.is_empty() {
            info!("No test clusters found to clean up");
            return Ok(());
        }

        info!(count = test_clusters.len(), "Found test clusters to clean up");

        // Clean up each test cluster
        for cluster in test_clusters {
            if let Err(e) = self.cleanup_infrastructure(cluster).await {
                warn!(cluster = cluster, error = e, "Failed to clean up cluster");
            }
        }

        info!("✅ All test clusters cleanup completed");
        Ok(())
    }
}

pub struct InfrastructureResult {
    pub cluster_name: String,
    pub client: Client,
    pub namespace: String,
}

impl std::fmt::Debug for InfrastructureResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InfrastructureResult")
            .field("cluster_name", &self.cluster_name)
            .field("namespace", &self.namespace)
            .field("client", &"[Kubernetes Client]")
            .finish()
    }
}

#[derive(Debug)]
pub struct ServiceResult {
    pub minio_endpoint: String,
    pub minio_access_key: String,
    pub minio_secret_key: String,
    pub operator_ready: bool,
}

#[derive(Debug)]
struct MinioResult {
    endpoint: String,
    access_key: String,
    secret_key: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_orchestrator_creation() {
        let orchestrator = ParallelSetupOrchestrator::new("test");
        assert!(orchestrator.cluster_name.starts_with("neon-e2e-test-"));
        assert_eq!(orchestrator.test_name, "test");
    }

    #[test]
    fn test_minio_yaml_generation() {
        let orchestrator = ParallelSetupOrchestrator::new("test");
        let yaml = orchestrator.generate_minio_yaml("test-ns", "user", "pass");

        assert!(yaml.contains("test-ns"));
        assert!(yaml.contains("user"));
        assert!(yaml.contains("pass"));
        assert!(yaml.contains("minio"));
    }

    #[tokio::test]
    async fn test_find_available_port() {
        let orchestrator = ParallelSetupOrchestrator::new("test");
        let port = orchestrator.find_available_port().await.unwrap();
        assert!(port > 1024); // Should be a high port
    }
}
