use super::resources::*;
use crate::util::errors::{Error, StdError};
use crate::util::{errors, errors::Result, metrics, telemetry};
use chrono::{DateTime, Utc};
use futures::StreamExt;
use k8s_openapi::api::{
    apps::v1::{Deployment, StatefulSet},
    core::v1::Service,
};
use kube::{
    api::{Api, ListParams, Patch, PatchParams, ResourceExt},
    client::Client,
    runtime::{
        controller::{Action, Controller},
        events::{Event, EventType, Recorder, Reporter},
        finalizer::{finalizer, Event as Finalizer},
        watcher::{self, Config},
    },
    Resource,
};
use rand::RngCore;
use serde::Serialize;
use serde_json::json;
use std::sync::Arc;
use tokio::{sync::RwLock, time::Duration};
use tracing::*;

pub const FIELD_MANAGER: &str = "neon-project-controller";

impl NeonProject {
    // Reconcile (for non-finalizer related changes)
    async fn reconcile(&self, ctx: Arc<Context>) -> Result<Action, errors::Error> {
        let namespace = self.namespace().unwrap();
        let name = self.name_any();

        let project_client: Api<NeonProject> = Api::namespaced(ctx.client.clone(), &namespace);

        if self.spec.tenant_id.is_none() {
            // Set field and set field manager for this field
            // tenant_id is a 32 character alphanumeric string
            let mut bytes = [0; 16];
            rand::thread_rng().fill_bytes(&mut bytes);
            let tenant_id = hex::encode(bytes);

            project_client
                .patch(
                    &name,
                    &PatchParams {
                        field_manager: Some("neon-operator".to_string()),
                        ..Default::default()
                    },
                    &Patch::Merge(json!({
                        "spec": {
                            "tenant_id": tenant_id
                        }
                    })),
                )
                .await
                .map_err(|e| Error::StdError(StdError::KubeError(e)))?;

            return Ok(Action::requeue(Duration::from_secs(1)));
        }

        // send http request to pageserver to ensure tenant is created
        let pageserver_url = format!(
            //"http://pageserver-{}.neon.svc.cluster.local:6400/v1/tenant/{}/location_config",
            "http://localhost:9898/v1/tenant/{}/location_config",
            //self.spec.cluster_name.clone(),
            self.spec.tenant_id.clone().unwrap()
        );
        let client = reqwest::Client::new();

        let response = client
            .put(pageserver_url)
            .header("Content-Type", "application/json")
            .body(r#"{"mode": "AttachedSingle", "generation": 1, "tenant_conf": {}}"#)
            .send()
            .await
            .unwrap();

        if !response.status().is_success() {
            println!("Failed to create tenant on pageserver: {:?}", response.status());
            return Ok(Action::requeue(Duration::from_secs(5)));
        }

        // If no events were received, check back every 5 minutes
        Ok(Action::requeue(Duration::from_secs(60)))
    }

    // Finalizer cleanup (the object was deleted, ensure nothing is orphaned)
    async fn cleanup(&self, ctx: Arc<Context>) -> Result<Action> {
        let recorder = ctx.diagnostics.read().await.recorder(ctx.client.clone(), self);
        // PRoject doesn't have any real cleanup, so we just publish an event
        recorder
            .publish(Event {
                type_: EventType::Normal,
                reason: "DeleteRequested".into(),
                note: Some(format!("Delete `{}`", self.name_any())),
                action: "Deleting".into(),
                secondary: None,
            })
            .await
            .map_err(|e| errors::Error::StdError(errors::StdError::KubeError(e)))?;
        Ok(Action::await_change())
    }
}

/// State shared between the controller and the web server
#[derive(Clone, Default)]
pub struct State {
    /// Diagnostics populated by the reconciler
    diagnostics: Arc<RwLock<Diagnostics>>,
    /// Metrics registry
    registry: prometheus::Registry,
}

/// State wrapper around the controller outputs for the web server
impl State {
    /// Metrics getter
    pub fn metrics(&self) -> Vec<prometheus::proto::MetricFamily> {
        self.registry.gather()
    }

    /// State getter
    pub async fn diagnostics(&self) -> Diagnostics {
        self.diagnostics.read().await.clone()
    }

    // Create a Controller Context that can update State
    pub fn to_context(&self, client: Client) -> Arc<Context> {
        Arc::new(Context {
            client,
            metrics: metrics::Metrics::default().register(&self.registry).unwrap(),
            diagnostics: self.diagnostics.clone(),
        })
    }
}

// Context for our reconciler
#[derive(Clone)]
pub struct Context {
    /// Kubernetes client
    pub client: Client,
    /// Diagnostics read by the web server
    pub diagnostics: Arc<RwLock<Diagnostics>>,
    /// Prometheus metrics
    pub metrics: metrics::Metrics,
}

#[instrument(skip(ctx, neon_project), fields(trace_id))]
pub async fn reconcile(neon_project: Arc<NeonProject>, ctx: Arc<Context>) -> Result<Action> {
    let trace_id = telemetry::get_trace_id();
    Span::current().record("trace_id", field::display(&trace_id));
    let _timer = ctx.metrics.count_and_measure("project");
    ctx.diagnostics.write().await.last_event = Utc::now();

    let ns = neon_project.namespace().unwrap(); // neon_project is namespace scoped
    let project_client: Api<NeonProject> = Api::namespaced(ctx.client.clone(), &ns);

    info!("Reconciling NeonProject\"{}\" in {}", neon_project.name_any(), ns);
    finalizer(
        &project_client,
        NEON_PROJECT_FINALIZER,
        neon_project.clone(),
        |event| async {
            match event {
                Finalizer::Apply(neon_project) => neon_project.reconcile(ctx.clone()).await,
                Finalizer::Cleanup(neon_project) => neon_project.cleanup(ctx.clone()).await,
            }
        },
    )
    .await
    .map_err(|e| errors::Error::StdError(errors::StdError::FinalizerError(Box::new(e))))?;

    Ok(Action::requeue(Duration::from_secs(5 * 60)))
}

/// Diagnostics to be exposed by the web server
#[derive(Clone, Serialize)]
pub struct Diagnostics {
    #[serde(deserialize_with = "from_ts")]
    pub last_event: DateTime<Utc>,
    #[serde(skip)]
    pub reporter: Reporter,
}
impl Default for Diagnostics {
    fn default() -> Self {
        Self {
            last_event: Utc::now(),
            reporter: "doc-controller".into(),
        }
    }
}
impl Diagnostics {
    fn recorder(&self, client: Client, neon_project: &NeonProject) -> Recorder {
        Recorder::new(client, self.reporter.clone(), neon_project.object_ref(&()))
    }
}

fn error_policy(neon_project: Arc<NeonProject>, error: &errors::Error, ctx: Arc<Context>) -> Action {
    warn!("reconcile failed: {:?}", error);
    ctx.metrics.reconcile_project_failure(&neon_project, error);
    Action::requeue(Duration::from_secs(5 * 60))
}

/// Initialize the controller and shared state (given the crd is installed)
pub async fn run(state: State) {
    let client = Client::try_default().await.expect("failed to create kube Client");

    let neonclusters = Api::<NeonProject>::all(client.clone());
    if let Err(e) = neonclusters.list(&ListParams::default().limit(1)).await {
        error!("CRD is not queryable; {e:?}. Is the CRD installed?");
        info!("Installation: cargo run --bin crdgen | kubectl apply -f -");
        std::process::exit(1);
    }

    Controller::new(neonclusters, Config::default().any_semantic())
        .owns(
            Api::<StatefulSet>::all(client.clone()),
            watcher::Config::default(),
        )
        .owns(Api::<Service>::all(client.clone()), watcher::Config::default())
        .owns(Api::<Deployment>::all(client.clone()), watcher::Config::default())
        .shutdown_on_signal()
        .run(reconcile, error_policy, state.to_context(client))
        .filter_map(|x| async move { std::result::Result::ok(x) })
        .for_each(|_| futures::future::ready(()))
        .await;
}
