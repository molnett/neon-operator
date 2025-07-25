use chrono::{DateTime, Utc};
use kube::{Api, Client};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, error, info, span, warn, Level, Span};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestMetrics {
    pub test_name: String,
    pub start_time: DateTime<Utc>,
    pub total_duration: Option<Duration>,
    pub phase_timings: HashMap<String, Duration>,
    pub component_ready_times: HashMap<String, Duration>,
    pub resource_counts: ResourceCounts,
    pub failure_events: Vec<FailureEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceCounts {
    pub pods_created: u32,
    pub services_created: u32,
    pub deployments_created: u32,
    pub crds_installed: u32,
    pub port_forwards: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureEvent {
    pub timestamp: DateTime<Utc>,
    pub component: String,
    pub error: String,
    pub context: HashMap<String, String>,
}

impl Default for ResourceCounts {
    fn default() -> Self {
        Self {
            pods_created: 0,
            services_created: 0,
            deployments_created: 0,
            crds_installed: 0,
            port_forwards: 0,
        }
    }
}

impl Default for TestMetrics {
    fn default() -> Self {
        let timestamp = Utc::now();

        Self {
            test_name: String::new(),
            start_time: timestamp,
            total_duration: None,
            phase_timings: HashMap::new(),
            component_ready_times: HashMap::new(),
            resource_counts: ResourceCounts::default(),
            failure_events: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub struct TestObserver {
    metrics: TestMetrics,
    phase_start_times: HashMap<String, DateTime<Utc>>,
    span: Span,
}

impl TestObserver {
    pub fn new(test_name: &str) -> Self {
        let span = span!(Level::INFO, "test_execution", test_name = test_name);

        info!(
            test_name = test_name,
            "🔍 Starting test observability for {}", test_name
        );

        let timestamp = Utc::now();

        Self {
            metrics: TestMetrics {
                test_name: test_name.to_string(),
                start_time: timestamp,
                ..Default::default()
            },
            phase_start_times: HashMap::new(),
            span,
        }
    }

    pub fn start_phase(&mut self, phase_name: &str) {
        let _enter = self.span.enter();
        self.phase_start_times.insert(phase_name.to_string(), Utc::now());

        info!(
            test_name = self.metrics.test_name,
            phase = phase_name,
            elapsed_ms = (Utc::now() - self.metrics.start_time).num_milliseconds().max(0) as u64,
            "🚀 Starting phase: {}",
            phase_name
        );
    }

    pub fn end_phase(&mut self, phase_name: &str) {
        let _enter = self.span.enter();

        if let Some(start_time) = self.phase_start_times.remove(phase_name) {
            let duration = Duration::from_millis((Utc::now() - start_time).num_milliseconds().max(0) as u64);
            self.metrics
                .phase_timings
                .insert(phase_name.to_string(), duration);

            info!(
                test_name = self.metrics.test_name,
                phase = phase_name,
                duration_ms = duration.as_millis(),
                total_elapsed_ms = (Utc::now() - self.metrics.start_time).num_milliseconds().max(0) as u64,
                "✅ Completed phase: {} in {:.1}s",
                phase_name,
                duration.as_secs_f64()
            );
        } else {
            warn!(
                test_name = self.metrics.test_name,
                phase = phase_name,
                "⚠️ Attempted to end phase that was never started: {}",
                phase_name
            );
        }
    }

    pub fn component_ready(&mut self, component: &str) {
        let _enter = self.span.enter();
        let duration =
            Duration::from_millis((Utc::now() - self.metrics.start_time).num_milliseconds().max(0) as u64);
        self.metrics
            .component_ready_times
            .insert(component.to_string(), duration);

        info!(
            test_name = self.metrics.test_name,
            component = component,
            ready_time_ms = duration.as_millis(),
            "🟢 Component ready: {} after {:.1}s",
            component,
            duration.as_secs_f64()
        );
    }

    pub fn record_failure(&mut self, component: &str, error: &str, context: HashMap<String, String>) {
        let _enter = self.span.enter();

        let timestamp = Utc::now();

        let failure = FailureEvent {
            timestamp,
            component: component.to_string(),
            error: error.to_string(),
            context,
        };

        self.metrics.failure_events.push(failure);

        error!(
            test_name = self.metrics.test_name,
            component = component,
            error = error,
            "❌ Failure recorded for component: {}",
            component
        );
    }

    pub fn increment_resource_count(&mut self, resource_type: &str) {
        let _enter = self.span.enter();

        match resource_type {
            "pod" => self.metrics.resource_counts.pods_created += 1,
            "service" => self.metrics.resource_counts.services_created += 1,
            "deployment" => self.metrics.resource_counts.deployments_created += 1,
            "crd" => self.metrics.resource_counts.crds_installed += 1,
            "port_forward" => self.metrics.resource_counts.port_forwards += 1,
            _ => {
                debug!(
                    test_name = self.metrics.test_name,
                    resource_type = resource_type,
                    "Unknown resource type for counting: {}",
                    resource_type
                );
            }
        }

        debug!(
            test_name = self.metrics.test_name,
            resource_type = resource_type,
            "📊 Incremented {} count",
            resource_type
        );
    }

    pub async fn observe_kubernetes_state(&self, client: &Client, namespace: &str) {
        let _enter = self.span.enter();

        info!(
            test_name = self.metrics.test_name,
            namespace = namespace,
            "🔍 Observing Kubernetes cluster state"
        );

        // Observe pods
        if let Err(e) = self.observe_pods(client, namespace).await {
            warn!(
                test_name = self.metrics.test_name,
                error = %e,
                "Failed to observe pods"
            );
        }

        // Observe deployments
        if let Err(e) = self.observe_deployments(client, namespace).await {
            warn!(
                test_name = self.metrics.test_name,
                error = %e,
                "Failed to observe deployments"
            );
        }

        // Observe services
        if let Err(e) = self.observe_services(client, namespace).await {
            warn!(
                test_name = self.metrics.test_name,
                error = %e,
                "Failed to observe services"
            );
        }
    }

    async fn observe_pods(&self, client: &Client, namespace: &str) -> Result<(), kube::Error> {
        use k8s_openapi::api::core::v1::Pod;

        let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);
        let pod_list = pods.list(&Default::default()).await?;

        let mut ready_count = 0;
        let mut pending_count = 0;
        let mut failed_count = 0;

        for pod in &pod_list.items {
            if let Some(status) = &pod.status {
                match status.phase.as_deref() {
                    Some("Running") => {
                        if let Some(conditions) = &status.conditions {
                            if conditions
                                .iter()
                                .any(|c| c.type_ == "Ready" && c.status == "True")
                            {
                                ready_count += 1;
                            }
                        }
                    }
                    Some("Pending") => pending_count += 1,
                    Some("Failed") => failed_count += 1,
                    _ => {}
                }
            }
        }

        info!(
            test_name = self.metrics.test_name,
            namespace = namespace,
            total_pods = pod_list.items.len(),
            ready_pods = ready_count,
            pending_pods = pending_count,
            failed_pods = failed_count,
            "📊 Pod status summary"
        );

        Ok(())
    }

    async fn observe_deployments(&self, client: &Client, namespace: &str) -> Result<(), kube::Error> {
        use k8s_openapi::api::apps::v1::Deployment;

        let deployments: Api<Deployment> = Api::namespaced(client.clone(), namespace);
        let deployment_list = deployments.list(&Default::default()).await?;

        for deployment in &deployment_list.items {
            if let Some(status) = &deployment.status {
                let name = deployment.metadata.name.as_deref().unwrap_or("unknown");
                let ready = status.ready_replicas.unwrap_or(0);
                let desired = status.replicas.unwrap_or(0);

                info!(
                    test_name = self.metrics.test_name,
                    deployment = name,
                    ready_replicas = ready,
                    desired_replicas = desired,
                    ready = (ready == desired && desired > 0),
                    "📊 Deployment status"
                );
            }
        }

        Ok(())
    }

    async fn observe_services(&self, client: &Client, namespace: &str) -> Result<(), kube::Error> {
        use k8s_openapi::api::core::v1::Service;

        let services: Api<Service> = Api::namespaced(client.clone(), namespace);
        let service_list = services.list(&Default::default()).await?;

        info!(
            test_name = self.metrics.test_name,
            namespace = namespace,
            service_count = service_list.items.len(),
            "📊 Service count"
        );

        for service in &service_list.items {
            let name = service.metadata.name.as_deref().unwrap_or("unknown");
            let service_type = service
                .spec
                .as_ref()
                .and_then(|s| s.type_.as_deref())
                .unwrap_or("ClusterIP");

            debug!(
                test_name = self.metrics.test_name,
                service = name,
                service_type = service_type,
                "📊 Service details"
            );
        }

        Ok(())
    }

    pub fn finish(&mut self) -> TestSummary {
        let _enter = self.span.enter();

        self.metrics.total_duration = Some(Duration::from_millis(
            (Utc::now() - self.metrics.start_time).num_milliseconds().max(0) as u64,
        ));

        let summary = TestSummary::from_metrics(&self.metrics);

        info!(
            test_name = self.metrics.test_name,
            total_duration_ms = self.metrics.total_duration.unwrap().as_millis(),
            success = summary.success,
            "🏁 Test execution completed"
        );

        summary.log_detailed_report();
        summary
    }

    pub fn get_metrics(&self) -> &TestMetrics {
        &self.metrics
    }
}

#[derive(Debug, Clone)]
pub struct TestSummary {
    pub test_name: String,
    pub success: bool,
    pub total_duration: Duration,
    pub phase_timings: HashMap<String, Duration>,
    pub component_ready_times: HashMap<String, Duration>,
    pub resource_counts: ResourceCounts,
    pub failure_count: usize,
    pub slowest_phase: Option<(String, Duration)>,
    pub slowest_component: Option<(String, Duration)>,
}

impl TestSummary {
    pub fn from_metrics(metrics: &TestMetrics) -> Self {
        let success = metrics.failure_events.is_empty();
        let total_duration = metrics.total_duration.unwrap_or_else(|| {
            Duration::from_millis((Utc::now() - metrics.start_time).num_milliseconds().max(0) as u64)
        });

        let slowest_phase = metrics
            .phase_timings
            .iter()
            .max_by_key(|(_, duration)| *duration)
            .map(|(name, duration)| (name.clone(), *duration));

        let slowest_component = metrics
            .component_ready_times
            .iter()
            .max_by_key(|(_, duration)| *duration)
            .map(|(name, duration)| (name.clone(), *duration));

        Self {
            test_name: metrics.test_name.clone(),
            success,
            total_duration,
            phase_timings: metrics.phase_timings.clone(),
            component_ready_times: metrics.component_ready_times.clone(),
            resource_counts: metrics.resource_counts.clone(),
            failure_count: metrics.failure_events.len(),
            slowest_phase,
            slowest_component,
        }
    }

    pub fn log_detailed_report(&self) {
        info!(
            test_name = self.test_name,
            "📊 ==================================="
        );
        info!(test_name = self.test_name, "📊 TEST EXECUTION SUMMARY");
        info!(
            test_name = self.test_name,
            "📊 ==================================="
        );

        info!(
            test_name = self.test_name,
            success = self.success,
            total_duration_ms = self.total_duration.as_millis(),
            total_duration_sec = format!("{:.2}", self.total_duration.as_secs_f64()),
            "📊 Overall Result: {} ({:.2}s)",
            if self.success { "✅ SUCCESS" } else { "❌ FAILED" },
            self.total_duration.as_secs_f64()
        );

        // Phase timings
        if !self.phase_timings.is_empty() {
            info!(test_name = self.test_name, "📊 Phase Timings:");

            let mut sorted_phases: Vec<_> = self.phase_timings.iter().collect();
            sorted_phases.sort_by_key(|(_, duration)| *duration);
            sorted_phases.reverse();

            for (phase, duration) in sorted_phases {
                let percentage = (duration.as_secs_f64() / self.total_duration.as_secs_f64()) * 100.0;
                info!(
                    test_name = self.test_name,
                    phase = phase,
                    duration_ms = duration.as_millis(),
                    percentage = format!("{:.1}%", percentage),
                    "📊   {} - {:.2}s ({:.1}%)",
                    phase,
                    duration.as_secs_f64(),
                    percentage
                );
            }
        }

        // Component ready times
        if !self.component_ready_times.is_empty() {
            info!(test_name = self.test_name, "📊 Component Ready Times:");

            let mut sorted_components: Vec<_> = self.component_ready_times.iter().collect();
            sorted_components.sort_by_key(|(_, duration)| *duration);

            for (component, duration) in sorted_components {
                info!(
                    test_name = self.test_name,
                    component = component,
                    ready_time_ms = duration.as_millis(),
                    "📊   {} - {:.2}s",
                    component,
                    duration.as_secs_f64()
                );
            }
        }

        // Resource counts
        info!(
            test_name = self.test_name,
            pods = self.resource_counts.pods_created,
            services = self.resource_counts.services_created,
            deployments = self.resource_counts.deployments_created,
            crds = self.resource_counts.crds_installed,
            port_forwards = self.resource_counts.port_forwards,
            "📊 Resources Created: {} pods, {} services, {} deployments, {} CRDs, {} port-forwards",
            self.resource_counts.pods_created,
            self.resource_counts.services_created,
            self.resource_counts.deployments_created,
            self.resource_counts.crds_installed,
            self.resource_counts.port_forwards
        );

        // Performance insights
        if let Some((phase, duration)) = &self.slowest_phase {
            info!(
                test_name = self.test_name,
                slowest_phase = phase,
                duration_ms = duration.as_millis(),
                "📊 Slowest Phase: {} ({:.2}s)",
                phase,
                duration.as_secs_f64()
            );
        }

        if let Some((component, duration)) = &self.slowest_component {
            info!(
                test_name = self.test_name,
                slowest_component = component,
                ready_time_ms = duration.as_millis(),
                "📊 Slowest Component: {} ({:.2}s)",
                component,
                duration.as_secs_f64()
            );
        }

        if self.failure_count > 0 {
            error!(
                test_name = self.test_name,
                failure_count = self.failure_count,
                "📊 ❌ {} failures occurred during test execution",
                self.failure_count
            );
        }

        info!(
            test_name = self.test_name,
            "📊 ==================================="
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_observer_creation() {
        let observer = TestObserver::new("test_case");
        assert_eq!(observer.metrics.test_name, "test_case");
        assert!(observer.metrics.phase_timings.is_empty());
    }

    #[test]
    fn test_phase_timing() {
        let mut observer = TestObserver::new("test_case");

        observer.start_phase("setup");
        std::thread::sleep(Duration::from_millis(10));
        observer.end_phase("setup");

        assert!(observer.metrics.phase_timings.contains_key("setup"));
        assert!(observer.metrics.phase_timings["setup"] >= Duration::from_millis(10));
    }

    #[test]
    fn test_resource_counting() {
        let mut observer = TestObserver::new("test_case");

        observer.increment_resource_count("pod");
        observer.increment_resource_count("service");
        observer.increment_resource_count("pod");

        assert_eq!(observer.metrics.resource_counts.pods_created, 2);
        assert_eq!(observer.metrics.resource_counts.services_created, 1);
    }

    #[test]
    fn test_failure_recording() {
        let mut observer = TestObserver::new("test_case");
        let mut context = HashMap::new();
        context.insert("namespace".to_string(), "test".to_string());

        observer.record_failure("component1", "test error", context);

        assert_eq!(observer.metrics.failure_events.len(), 1);
        assert_eq!(observer.metrics.failure_events[0].component, "component1");
        assert_eq!(observer.metrics.failure_events[0].error, "test error");
    }

    #[test]
    fn test_summary_creation() {
        let mut observer = TestObserver::new("test_case");

        observer.start_phase("setup");
        std::thread::sleep(Duration::from_millis(10));
        observer.end_phase("setup");

        observer.component_ready("test-component");

        let summary = observer.finish();

        assert_eq!(summary.test_name, "test_case");
        assert!(summary.success);
        assert!(!summary.phase_timings.is_empty());
        assert!(!summary.component_ready_times.is_empty());
    }
}
