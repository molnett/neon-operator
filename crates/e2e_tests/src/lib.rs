// Modern E2E Test Framework for Neon Kubernetes Operator
// Provides deterministic, fast, and observable testing with state machine orchestration

use kube::Client;
use std::time::Duration;
use tracing::info;

// Re-export all modules for easy access
pub mod observability;
pub mod parallel_setup;
pub mod state_machine;
pub mod waiting;

pub use observability::{TestMetrics, TestObserver, TestSummary};
pub use parallel_setup::{InfrastructureResult, ParallelSetupOrchestrator, ServiceResult};
pub use state_machine::{TestState, TestStateMachine};
pub use waiting::{with_progress_indicator, BackoffConfig, SmartWaiter};

/// Modern test environment using state machine orchestration and parallel setup
pub struct TestEnv {
    pub cluster_name: String,
    pub client: Client,
    pub namespace: String,
    pub minio_endpoint: String,
    pub minio_access_key: String,
    pub minio_secret_key: String,
    observer: TestObserver,
    state_machine: TestStateMachine,
    config: TestConfig,
}

impl std::fmt::Debug for TestEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestEnv")
            .field("cluster_name", &self.cluster_name)
            .field("namespace", &self.namespace)
            .field("minio_endpoint", &self.minio_endpoint)
            .field("minio_access_key", &"[REDACTED]")
            .field("minio_secret_key", &"[REDACTED]")
            .finish()
    }
}

/// Result type for test environment operations
pub type TestResult<T> = Result<T, TestError>;

/// Enhanced error type for better error reporting
#[derive(Debug)]
pub enum TestError {
    Infrastructure(String),
    Kubernetes(String),
    Timeout(String),
    Setup(String),
    Cleanup(String),
}

impl std::fmt::Display for TestError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            TestError::Infrastructure(msg) => write!(f, "Infrastructure error: {}", msg),
            TestError::Kubernetes(msg) => write!(f, "Kubernetes error: {}", msg),
            TestError::Timeout(msg) => write!(f, "Timeout error: {}", msg),
            TestError::Setup(msg) => write!(f, "Setup error: {}", msg),
            TestError::Cleanup(msg) => write!(f, "Cleanup error: {}", msg),
        }
    }
}

impl std::error::Error for TestError {}

impl TestEnv {
    /// Create a new test environment using modern parallel setup orchestration
    pub async fn new(test_name: &str) -> TestResult<Self> {
        Self::with_config(test_name, TestConfig::default()).await
    }

    /// Create a test environment with custom configuration
    pub async fn with_config(test_name: &str, config: TestConfig) -> TestResult<Self> {
        let mut observer = TestObserver::new(test_name);
        let mut state_machine = TestStateMachine::new(test_name);

        observer.start_phase("initialization");

        info!(
            test_name = test_name,
            "🚀 Creating test environment with modern parallel setup"
        );

        // Use the new parallel setup orchestrator with config-driven behavior
        let orchestrator = ParallelSetupOrchestrator::new(test_name);

        // Phase 1: Infrastructure setup (cluster + CRDs)
        observer.start_phase("infrastructure_setup");
        let infrastructure = if config.parallel_setup {
            orchestrator
                .setup_infrastructure(&mut state_machine)
                .await
                .map_err(|e| TestError::Infrastructure(e))?
        } else {
            // Sequential setup for debugging or slower environments
            orchestrator
                .setup_infrastructure_sequential(&mut state_machine)
                .await
                .map_err(|e| TestError::Infrastructure(e))?
        };
        observer.end_phase("infrastructure_setup");
        observer.component_ready("infrastructure");

        // Phase 2: Service deployment (MinIO + Operator)
        observer.start_phase("service_deployment");
        let services = if config.parallel_setup {
            orchestrator
                .deploy_services(
                    &infrastructure.client,
                    &infrastructure.namespace,
                    &mut state_machine,
                )
                .await
                .map_err(|e| TestError::Setup(e))?
        } else {
            orchestrator
                .deploy_services_sequential(
                    &infrastructure.client,
                    &infrastructure.namespace,
                    &mut state_machine,
                )
                .await
                .map_err(|e| TestError::Setup(e))?
        };
        observer.end_phase("service_deployment");
        observer.component_ready("services");

        // Observe cluster state if observability is enabled
        if config.observability_enabled {
            observer
                .observe_kubernetes_state(&infrastructure.client, &infrastructure.namespace)
                .await;
        }

        observer.end_phase("initialization");

        state_machine
            .transition_to(TestState::ComponentsReady)
            .map_err(|e| TestError::Setup(e))?;

        info!(
            test_name = test_name,
            cluster_name = infrastructure.cluster_name,
            namespace = infrastructure.namespace,
            "✅ Test environment ready with enhanced observability"
        );

        let test_env = TestEnv {
            cluster_name: infrastructure.cluster_name,
            client: infrastructure.client,
            namespace: infrastructure.namespace,
            minio_endpoint: services.minio_endpoint,
            minio_access_key: services.minio_access_key,
            minio_secret_key: services.minio_secret_key,
            observer,
            state_machine,
            config: config.clone(),
        };

        Ok(test_env)
    }

    /// Get the test observer for metrics and observability
    pub fn observer(&self) -> &TestObserver {
        &self.observer
    }

    /// Get the state machine for state tracking
    pub fn state_machine(&self) -> &TestStateMachine {
        &self.state_machine
    }

    /// Start observing a test phase
    pub fn start_phase(&mut self, phase_name: &str) {
        self.observer.start_phase(phase_name);
    }

    /// End observing a test phase
    pub fn end_phase(&mut self, phase_name: &str) {
        self.observer.end_phase(phase_name);
    }

    /// Mark a component as ready
    pub fn component_ready(&mut self, component: &str) {
        self.observer.component_ready(component);
    }

    /// Record a failure event
    pub fn record_failure(&mut self, component: &str, error: &str) {
        let mut context = std::collections::HashMap::new();
        context.insert("cluster_name".to_string(), self.cluster_name.clone());
        context.insert("namespace".to_string(), self.namespace.clone());
        self.observer.record_failure(component, error, context);
    }

    /// Observe current Kubernetes state
    pub async fn observe_state(&self) {
        self.observer
            .observe_kubernetes_state(&self.client, &self.namespace)
            .await;
    }

    /// Clean up the test environment
    pub async fn cleanup(&mut self) -> TestResult<TestSummary> {
        self.start_phase("cleanup");

        info!(
            test_name = self.observer.get_metrics().test_name,
            cluster_name = self.cluster_name,
            "🧹 Starting test environment cleanup"
        );

        // Use the parallel setup orchestrator for cleanup
        let orchestrator = ParallelSetupOrchestrator::new(&self.observer.get_metrics().test_name);
        if let Err(e) = orchestrator.cleanup_infrastructure(&self.cluster_name).await {
            if self.config.cleanup_on_failure {
                self.record_failure("cleanup", &e);
            }
        }

        self.end_phase("cleanup");

        let summary = self.observer.finish();
        Ok(summary)
    }

    /// Transition to a new test state
    pub fn transition_to(&mut self, state: TestState) -> TestResult<()> {
        self.state_machine
            .transition_to(state)
            .map_err(|e| TestError::Setup(e))
    }

    /// Get current test state
    pub fn current_state(&self) -> &TestState {
        self.state_machine.current_state()
    }
}

/// Configuration for test environment creation
#[derive(Debug, Clone)]
pub struct TestConfig {
    pub timeout: Duration,
    pub parallel_setup: bool,
    pub observability_enabled: bool,
    pub cleanup_on_failure: bool,
}

impl Default for TestConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(300), // 5 minutes default timeout
            parallel_setup: true,
            observability_enabled: true,
            cleanup_on_failure: true,
        }
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        // Schedule cluster cleanup (fire and forget) only if configured to do so
        if self.config.cleanup_on_failure {
            let cluster_name = self.cluster_name.clone();
            tokio::spawn(async move {
                let orchestrator = ParallelSetupOrchestrator::new("auto-cleanup");
                if let Err(e) = orchestrator.cleanup_infrastructure(&cluster_name).await {
                    tracing::warn!("Failed to cleanup Kind cluster {}: {}", cluster_name, e);
                }
            });
        }
    }
}

// Add a cleanup function that can be called from panic hooks or signal handlers
pub async fn cleanup_all_test_clusters() -> Result<(), Box<dyn std::error::Error>> {
    info!("Cleaning up all e2e test clusters");

    let orchestrator = ParallelSetupOrchestrator::new("global-cleanup");
    orchestrator
        .cleanup_all_test_clusters()
        .await
        .map_err(|e| e.into())
}

// Helper function to wait for resource conditions (uses the new SmartWaiter)
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
    let waiter = SmartWaiter::with_config(
        &format!("wait_for_condition_{}_{}", name, condition_type),
        BackoffConfig {
            initial_delay: Duration::from_secs(2),
            max_delay: Duration::from_secs(10),
            multiplier: 1.5,
            max_attempts: (timeout.as_secs() / 2) as u32,
        },
    );

    waiter
        .wait_for_resource_condition::<T>(client, namespace, name, condition_type, condition_status)
        .await
        .map_err(|e| e.into())
}
