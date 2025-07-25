use kube::{Api, Client, ResourceExt};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{debug, info, warn};

const MAX_BACKOFF: Duration = Duration::from_secs(15);
const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const BACKOFF_MULTIPLIER: f64 = 2.0;

#[derive(Debug, Clone)]
pub struct BackoffConfig {
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub multiplier: f64,
    pub max_attempts: u32,
}

impl Default for BackoffConfig {
    fn default() -> Self {
        Self {
            initial_delay: INITIAL_BACKOFF,
            max_delay: MAX_BACKOFF,
            multiplier: BACKOFF_MULTIPLIER,
            max_attempts: 30, // Up to ~7.5 minutes with default config
        }
    }
}

impl BackoffConfig {
    pub fn fast() -> Self {
        Self {
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(5),
            multiplier: 1.5,
            max_attempts: 20,
        }
    }

    pub fn slow() -> Self {
        Self {
            initial_delay: Duration::from_secs(2),
            max_delay: Duration::from_secs(30),
            multiplier: 2.0,
            max_attempts: 50,
        }
    }
}

pub struct SmartWaiter {
    config: BackoffConfig,
    context: String,
}

impl SmartWaiter {
    pub fn new(context: &str) -> Self {
        Self {
            config: BackoffConfig::default(),
            context: context.to_string(),
        }
    }

    pub fn with_config(context: &str, config: BackoffConfig) -> Self {
        Self {
            config,
            context: context.to_string(),
        }
    }

    pub async fn wait_for<F, Fut, T>(&self, mut condition: F) -> Result<T, String>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<Option<T>, String>>,
    {
        let start_time = Instant::now();
        let mut delay = self.config.initial_delay;
        let mut attempt = 1;

        info!(
            context = self.context,
            max_attempts = self.config.max_attempts,
            initial_delay_ms = self.config.initial_delay.as_millis(),
            max_delay_ms = self.config.max_delay.as_millis(),
            "🔄 Starting smart wait"
        );

        loop {
            debug!(
                context = self.context,
                attempt = attempt,
                delay_ms = delay.as_millis(),
                elapsed_ms = start_time.elapsed().as_millis(),
                "Checking condition"
            );

            match condition().await {
                Ok(Some(result)) => {
                    info!(
                        context = self.context,
                        attempts = attempt,
                        total_duration_ms = start_time.elapsed().as_millis(),
                        "✅ Condition satisfied"
                    );
                    return Ok(result);
                }
                Ok(None) => {
                    debug!(
                        context = self.context,
                        attempt = attempt,
                        "Condition not yet satisfied, continuing..."
                    );
                }
                Err(e) => {
                    warn!(
                        context = self.context,
                        attempt = attempt,
                        error = e,
                        "Error checking condition"
                    );
                    // Continue trying on errors (might be transient)
                }
            }

            if attempt >= self.config.max_attempts {
                let total_time = start_time.elapsed();
                return Err(format!(
                    "{}: Condition not satisfied after {} attempts ({:.1}s)",
                    self.context,
                    attempt,
                    total_time.as_secs_f64()
                ));
            }

            sleep(delay).await;

            // Calculate next delay with exponential backoff
            delay = std::cmp::min(
                Duration::from_millis((delay.as_millis() as f64 * self.config.multiplier) as u64),
                self.config.max_delay,
            );

            attempt += 1;
        }
    }

    pub async fn wait_for_deployment_ready(
        &self,
        client: &Client,
        namespace: &str,
        name: &str,
    ) -> Result<(), String> {
        use k8s_openapi::api::apps::v1::Deployment;

        let api: Api<Deployment> = Api::namespaced(client.clone(), namespace);

        self.wait_for(|| {
            let api = api.clone();
            let name = name.to_string();
            async move {
                match api.get(&name).await {
                    Ok(deployment) => {
                        if let Some(status) = &deployment.status {
                            if let (Some(ready_replicas), Some(replicas)) =
                                (status.ready_replicas, status.replicas)
                            {
                                if ready_replicas == replicas && replicas > 0 {
                                    debug!(
                                        deployment = name,
                                        ready_replicas = ready_replicas,
                                        total_replicas = replicas,
                                        "Deployment ready"
                                    );
                                    return Ok(Some(()));
                                }
                            }
                            debug!(
                                deployment = name,
                                ready_replicas = status.ready_replicas,
                                total_replicas = status.replicas,
                                "Deployment not ready yet"
                            );
                        }
                        Ok(None)
                    }
                    Err(e) => Err(format!("Failed to get deployment {}: {}", name, e)),
                }
            }
        })
        .await
    }

    pub async fn wait_for_service_endpoints(
        &self,
        client: &Client,
        namespace: &str,
        service_name: &str,
    ) -> Result<(), String> {
        use k8s_openapi::api::core::v1::Endpoints;

        let api: Api<Endpoints> = Api::namespaced(client.clone(), namespace);

        self.wait_for(|| {
            let api = api.clone();
            let service_name = service_name.to_string();
            async move {
                match api.get(&service_name).await {
                    Ok(endpoints) => {
                        if let Some(subsets) = &endpoints.subsets {
                            for subset in subsets {
                                if let Some(addresses) = &subset.addresses {
                                    if !addresses.is_empty() {
                                        debug!(
                                            service = service_name,
                                            endpoint_count = addresses.len(),
                                            "Service has ready endpoints"
                                        );
                                        return Ok(Some(()));
                                    }
                                }
                            }
                        }
                        debug!(service = service_name, "Service has no ready endpoints yet");
                        Ok(None)
                    }
                    Err(e) => Err(format!("Failed to get endpoints for {}: {}", service_name, e)),
                }
            }
        })
        .await
    }

    pub async fn wait_for_pod_ready(
        &self,
        client: &Client,
        namespace: &str,
        label_selector: &str,
    ) -> Result<String, String> {
        use k8s_openapi::api::core::v1::Pod;

        let api: Api<Pod> = Api::namespaced(client.clone(), namespace);

        self.wait_for(|| {
            let api = api.clone();
            let label_selector = label_selector.to_string();
            async move {
                let list_params = kube::api::ListParams::default().labels(&label_selector);

                match api.list(&list_params).await {
                    Ok(pod_list) => {
                        for pod in &pod_list.items {
                            if let Some(status) = &pod.status {
                                if let Some(phase) = &status.phase {
                                    if phase == "Running" {
                                        if let Some(conditions) = &status.conditions {
                                            for condition in conditions {
                                                if condition.type_ == "Ready" && condition.status == "True" {
                                                    let pod_name = pod.name_any();
                                                    debug!(pod = pod_name, phase = phase, "Pod ready");
                                                    return Ok(Some(pod_name));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        debug!(
                            label_selector = label_selector,
                            pod_count = pod_list.items.len(),
                            "No ready pods found yet"
                        );
                        Ok(None)
                    }
                    Err(e) => Err(format!("Failed to list pods: {}", e)),
                }
            }
        })
        .await
    }

    pub async fn wait_for_http_endpoint(
        &self,
        url: &str,
        expected_status: Option<u16>,
    ) -> Result<(), String> {
        let client = reqwest::Client::new();
        let expected_status = expected_status.unwrap_or(200);

        self.wait_for(|| {
            let client = client.clone();
            let url = url.to_string();
            async move {
                match client.get(&url).send().await {
                    Ok(response) => {
                        if response.status().as_u16() == expected_status {
                            debug!(
                                url = url,
                                status = response.status().as_u16(),
                                "HTTP endpoint ready"
                            );
                            Ok(Some(()))
                        } else {
                            debug!(
                                url = url,
                                status = response.status().as_u16(),
                                expected = expected_status,
                                "HTTP endpoint returned unexpected status"
                            );
                            Ok(None)
                        }
                    }
                    Err(e) => {
                        debug!(
                            url = url,
                            error = %e,
                            "HTTP endpoint not reachable yet"
                        );
                        Ok(None) // Don't fail on connection errors, keep trying
                    }
                }
            }
        })
        .await
    }

    pub async fn wait_for_resource_condition<T>(
        &self,
        client: &Client,
        namespace: &str,
        name: &str,
        condition_type: &str,
        condition_status: &str,
    ) -> Result<(), String>
    where
        T: kube::Resource<Scope = kube::core::NamespaceResourceScope>
            + serde::de::DeserializeOwned
            + serde::Serialize
            + Clone
            + std::fmt::Debug,
        <T as kube::Resource>::DynamicType: Default,
    {
        let api: Api<T> = Api::namespaced(client.clone(), namespace);

        self.wait_for(|| {
            let api = api.clone();
            let name = name.to_string();
            let condition_type = condition_type.to_string();
            let condition_status = condition_status.to_string();

            async move {
                match api.get(&name).await {
                    Ok(resource) => {
                        let resource_value = match serde_json::to_value(&resource) {
                            Ok(v) => v,
                            Err(e) => return Err(format!("Failed to serialize resource: {}", e)),
                        };

                        if let Some(status) = resource_value.get("status") {
                            if let Some(conditions) = status.get("conditions") {
                                if let Some(conditions_array) = conditions.as_array() {
                                    for condition in conditions_array {
                                        if let (Some(cond_type), Some(cond_status)) = (
                                            condition.get("type").and_then(|v| v.as_str()),
                                            condition.get("status").and_then(|v| v.as_str()),
                                        ) {
                                            if cond_type == condition_type && cond_status == condition_status
                                            {
                                                debug!(
                                                    resource = name,
                                                    condition_type = condition_type,
                                                    condition_status = condition_status,
                                                    "Resource condition satisfied"
                                                );
                                                return Ok(Some(()));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        debug!(
                            resource = name,
                            condition_type = condition_type,
                            condition_status = condition_status,
                            "Resource condition not satisfied yet"
                        );
                        Ok(None)
                    }
                    Err(e) => Err(format!("Failed to get resource {}: {}", name, e)),
                }
            }
        })
        .await
    }

    pub async fn wait_for_crd_established(&self, client: &Client, crd_name: &str) -> Result<(), String> {
        use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;

        let api: Api<CustomResourceDefinition> = Api::all(client.clone());

        self.wait_for(|| {
            let api = api.clone();
            let crd_name = crd_name.to_string();
            async move {
                match api.get(&crd_name).await {
                    Ok(crd) => {
                        if let Some(status) = &crd.status {
                            if let Some(conditions) = &status.conditions {
                                for condition in conditions {
                                    if condition.type_ == "Established" && condition.status == "True" {
                                        debug!(crd = crd_name, "CRD established");
                                        return Ok(Some(()));
                                    }
                                }
                            }
                        }
                        debug!(crd = crd_name, "CRD not established yet");
                        Ok(None)
                    }
                    Err(e) => Err(format!("Failed to get CRD {}: {}", crd_name, e)),
                }
            }
        })
        .await
    }
}

// Helper function for creating progress indicators during long operations
pub async fn with_progress_indicator<F, T>(operation_name: &str, estimated_duration: Duration, future: F) -> T
where
    F: std::future::Future<Output = T>,
{
    let start = Instant::now();
    let progress_interval = Duration::from_secs(5);

    info!(
        operation = operation_name,
        estimated_duration_ms = estimated_duration.as_millis(),
        "🚀 Starting operation"
    );

    tokio::select! {
        result = future => {
            let actual_duration = start.elapsed();
            info!(
                operation = operation_name,
                actual_duration_ms = actual_duration.as_millis(),
                estimated_duration_ms = estimated_duration.as_millis(),
                "✅ Operation completed"
            );
            result
        }
        _ = async {
            loop {
                sleep(progress_interval).await;
                let elapsed = start.elapsed();
                let progress = if estimated_duration > Duration::ZERO {
                    (elapsed.as_secs_f64() / estimated_duration.as_secs_f64() * 100.0).min(95.0)
                } else {
                    0.0
                };

                debug!(
                    operation = operation_name,
                    elapsed_ms = elapsed.as_millis(),
                    estimated_progress = format!("{:.1}%", progress),
                    "📊 Operation in progress"
                );
            }
        } => unreachable!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_smart_waiter_success() {
        let waiter = SmartWaiter::new("test");
        let counter = Arc::new(AtomicU32::new(0));

        let result = waiter
            .wait_for(|| {
                let counter = counter.clone();
                async move {
                    let count = counter.fetch_add(1, Ordering::SeqCst);
                    if count >= 2 {
                        Ok(Some("success".to_string()))
                    } else {
                        Ok(None)
                    }
                }
            })
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "success");
    }

    #[tokio::test]
    async fn test_smart_waiter_timeout() {
        let waiter = SmartWaiter::with_config(
            "test",
            BackoffConfig {
                initial_delay: Duration::from_millis(10),
                max_delay: Duration::from_millis(50),
                multiplier: 2.0,
                max_attempts: 3,
            },
        );

        let result = waiter
            .wait_for(|| async {
                Ok::<Option<()>, String>(None) // Never succeeds
            })
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("3 attempts"));
    }

    #[tokio::test]
    async fn test_progress_indicator() {
        let start = Instant::now();

        let result = with_progress_indicator("test_operation", Duration::from_millis(100), async {
            sleep(Duration::from_millis(50)).await;
            "completed"
        })
        .await;

        assert_eq!(result, "completed");
        assert!(start.elapsed() >= Duration::from_millis(50));
    }
}
