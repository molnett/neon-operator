use crate::api::v1alpha1::neonpageserver::{NeonPageserver, NEON_PAGESERVER_FINALIZER};
use crate::pageserver;
use crate::util::pageserver_status::PageserverStatusManager;
use crate::util::{errors, errors::Result, metrics};
use chrono::{DateTime, Utc};
use futures::StreamExt;
use k8s_openapi::api::apps::v1::StatefulSet;
use k8s_openapi::api::core::v1::Service;
use kube::{
    api::{Api, ListParams, ResourceExt},
    client::Client,
    runtime::{
        controller::{Action, Controller},
        events::{Event, EventType, Recorder, Reporter},
        finalizer::{finalizer, Event as Finalizer},
        watcher::{self, Config},
    },
    Resource,
};
use serde::Serialize;
use std::sync::Arc;
use tokio::{sync::RwLock, time::Duration};
use tracing::*;

pub const FIELD_MANAGER: &str = "neon-pageserver-controller";

impl NeonPageserver {
    // Reconcile (for non-finalizer related changes)
    async fn reconcile(&self, ctx: Arc<Context>) -> Result<Action, errors::Error> {
        // Initialize status manager
        let status_manager = PageserverStatusManager::new(&ctx.client, self)?;

        // Initialize status if not present
        if self.status.is_none() {
            status_manager
                .update_phase(crate::util::pageserver_status::PageserverPhase::Creating)
                .await?;
            status_manager.set_pageserver_ready(false).await?;
        }

        pageserver::reconcile(self, ctx).await?;

        status_manager.set_pageserver_ready(true).await?;

        // If no events were received, check back every minute
        Ok(Action::await_change())
    }

    // Finalizer cleanup (the object was deleted, ensure nothing is orphane)
    async fn cleanup(&self, ctx: Arc<Context>) -> Result<Action> {
        let recorder = ctx.diagnostics.read().await.recorder(ctx.client.clone());
        // PRoject doesn't have any real cleanup, so we just publish an event
        recorder
            .publish(
                &Event {
                    type_: EventType::Normal,
                    reason: "DeleteRequested".into(),
                    note: Some(format!("Delete `{}`", self.name_any())),
                    action: "Deleting".into(),
                    secondary: None,
                },
                &self.object_ref(&()),
            )
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

pub async fn reconcile(neon_pageserver: Arc<NeonPageserver>, ctx: Arc<Context>) -> Result<Action> {
    let _timer = ctx.metrics.count_and_measure("pageserver");
    ctx.diagnostics.write().await.last_event = Utc::now();

    let ns = neon_pageserver.namespace().unwrap();
    let pageserver_client: Api<NeonPageserver> = Api::namespaced(ctx.client.clone(), &ns);

    info!(
        "Reconciling NeonPageserver\"{}\" in {}",
        neon_pageserver.name_any(),
        ns
    );
    finalizer(
        &pageserver_client,
        NEON_PAGESERVER_FINALIZER,
        neon_pageserver.clone(),
        |event| async {
            match event {
                Finalizer::Apply(neon_pageserver) => neon_pageserver.reconcile(ctx.clone()).await,
                Finalizer::Cleanup(neon_pageserver) => neon_pageserver.cleanup(ctx.clone()).await,
            }
        },
    )
    .await
    .map_err(|e| errors::Error::StdError(errors::StdError::FinalizerError(Box::new(e))))
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
    fn recorder(&self, client: Client) -> Recorder {
        Recorder::new(client, self.reporter.clone())
    }
}

fn error_policy(neon_pageserver: Arc<NeonPageserver>, error: &errors::Error, ctx: Arc<Context>) -> Action {
    warn!("reconcile failed: {:?}", error);
    ctx.metrics.reconcile_pageserver_failure(&neon_pageserver, error);
    Action::requeue(Duration::from_secs(5 * 60))
}

/// Initialize the controller and shared state (given the crd is installed)
pub async fn run(state: State) {
    let client = Client::try_default().await.expect("failed to create kube Client");

    let neonpageservers = Api::<NeonPageserver>::all(client.clone());
    if let Err(e) = neonpageservers.list(&ListParams::default().limit(1)).await {
        error!("CRD is not queryable; {e:?}. Is the CRD installed?");
        info!("Installation: cargo run --bin crdgen | kubectl apply -f -");
        std::process::exit(1);
    }

    Controller::new(neonpageservers, Config::default().any_semantic())
        .owns(Api::<Service>::all(client.clone()), watcher::Config::default())
        .owns(
            Api::<StatefulSet>::all(client.clone()),
            watcher::Config::default(),
        )
        .run(reconcile, error_policy, state.to_context(client))
        .filter_map(|x| async move { std::result::Result::ok(x) })
        .for_each(|_| futures::future::ready(()))
        .await;
}
