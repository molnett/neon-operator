use crate::controllers::{pageserver, safekeeper, storage_broker};
use chrono::{DateTime, Utc};
use futures::StreamExt;
use kube::{
    api::{Api, ListParams, Patch, PatchParams, ResourceExt},
    client::Client,
    runtime::{
        controller::{Action, Controller},
        events::{Event, EventType, Recorder, Reporter},
        finalizer::{finalizer, Event as Finalizer},
        watcher::Config,
    },
    CustomResource, Resource,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tokio::{sync::RwLock, time::Duration};
use tracing::*;

use crate::util::{errors, errors::{Result}, metrics, telemetry};

pub static NEON_STORAGE_FINALIZER: &str = "neon-storage.oltp.molnett.org";

/// Generate the Kubernetes wrapper struct `NeonStorage` from our Spec and Status struct
///
/// This provides a hook for generating the CRD yaml (in crdgen.rs)
#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[cfg_attr(test, derive(Default))]
#[kube(kind = "NeonStorage", group = "oltp.molnett.org", version = "v1", namespaced)]
#[kube(status = "NeonStorageStatus", shortname = "neonstorage")]
pub struct NeonStorageSpec {
    pub hide: bool,

    pub pg_version: String,
    pub page_server: PageServer,
    pub safekeeper: SafeKeeper,
    pub storage_broker: StorageBroker,
}
/// The status object of `NeonStorage`
#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct NeonStorageStatus {
    pub hidden: bool,

    pub page_server_status: NeonStoragePageServerStatus,
    pub storage_broker_status: NeonStorageStorageBrokerStatus,
    pub safekeeper_status: NeonStorageSafeKeeperStatus,
}

/// The status object of `NeonStorage`
#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct NeonStoragePageServerStatus {}

/// The status object of `NeonStorage`
#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct NeonStorageStorageBrokerStatus {}

/// The status object of `NeonStorage`
#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct NeonStorageSafeKeeperStatus {}

// The configuration for the pageserver
#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct PageServer {
    pub id: u32,
}

// The configuration for the safekeepers
#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct SafeKeeper {
    pub replicas: u32,
}

// The configuration for the storage_broker
#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct StorageBroker {
    pub replicas: u32,
}

impl NeonStorage {
    fn was_hidden(&self) -> bool {
        self.status.as_ref().map(|s| s.hidden).unwrap_or(false)
    }
}

impl NeonStorage {
    // Reconcile (for non-finalizer related changes)
    async fn reconcile(&self, ctx: Arc<Context>) -> Result<Action, errors::Error> {
        let client = ctx.client.clone();
        let recorder = ctx.diagnostics.read().await.recorder(client.clone(), self);
        let ns = self.namespace().unwrap();
        let name = self.name_any();
        let docs: Api<NeonStorage> = Api::namespaced(client, &ns);

        let should_hide = self.spec.hide;
        if !self.was_hidden() && should_hide {
            // send an event once per hide
            recorder
                .publish(Event {
                    type_: EventType::Normal,
                    reason: "HideRequested".into(),
                    note: Some(format!("Hiding `{name}`")),
                    action: "Hiding".into(),
                    secondary: None,
                })
                .await
                .map_err(|e| errors::Error::StdError(errors::StdError::KubeError(e)))?;
        }
        if name == "illegal" {
            return Err(errors::Error::StdError(errors::StdError::IllegalDocument));
            // error names show up in metrics
        }
        // always overwrite status object with what we saw
        let new_status = Patch::Apply(json!({
            "apiVersion": "oltp.molnett.org/v1",
            "kind": "NeonStorage",
            "status": NeonStorageStatus {
                page_server_status: NeonStoragePageServerStatus{},
                storage_broker_status: NeonStorageStorageBrokerStatus{},
                safekeeper_status: NeonStorageSafeKeeperStatus{},
                hidden: should_hide,
            }
        }));
        let ps = PatchParams::apply("cntrlr").force();
        let _o = docs
            .patch_status(&name, &ps, &new_status)
            .await
            .map_err(|e| errors::Error::StdError(errors::StdError::KubeError(e)))?;

        // If no events were received, check back every 5 minutes
        Ok(Action::requeue(Duration::from_secs(5 * 60)))
    }

    // Finalizer cleanup (the object was deleted, ensure nothing is orphaned)
    async fn cleanup(&self, ctx: Arc<Context>) -> Result<Action> {
        let recorder = ctx.diagnostics.read().await.recorder(ctx.client.clone(), self);
        // Document doesn't have any real cleanup, so we just publish an event
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

#[instrument(skip(ctx, neon_storage), fields(trace_id))]
pub async fn reconcile(neon_storage: Arc<NeonStorage>, ctx: Arc<Context>) -> Result<Action> {
    let trace_id = telemetry::get_trace_id();
    Span::current().record("trace_id", &field::display(&trace_id));
    let _timer = ctx.metrics.count_and_measure();
    ctx.diagnostics.write().await.last_event = Utc::now();

    let ns = neon_storage.namespace().unwrap(); // neon_storage is namespace scoped
    let neon_storages: Api<NeonStorage> = Api::namespaced(ctx.client.clone(), &ns);

    info!(
        "Reconciling neon_storageument \"{}\" in {}",
        neon_storage.name_any(),
        ns
    );
    finalizer(
        &neon_storages,
        NEON_STORAGE_FINALIZER,
        neon_storage.clone(),
        |event| async {
            match event {
                Finalizer::Apply(neon_storage) => neon_storage.reconcile(ctx.clone()).await,
                Finalizer::Cleanup(neon_storage) => neon_storage.cleanup(ctx.clone()).await,
            }
        },
    )
    .await
    .map_err(|e| errors::Error::StdError(errors::StdError::FinalizerError(Box::new(e))));

    // first reconcile storage broker
    match storage_broker::reconcile(neon_storage.clone(), ctx.clone()).await {
        Ok(_) => (),
        Err(e) => {
            error!("failed to reconcile storage broker: {}", e);
            match e {
                errors::Error::ErrorWithRequeue(error) => return Ok(Action::requeue(error.duration)),
                other => return Ok(Action::await_change()),
            }
        }
    }

    // then reconcile safekeeper
    match safekeeper::reconcile(neon_storage.clone(), ctx.clone()).await {
        Ok(_) => (),
        Err(e) => {
            error!("failed to reconcile safekeeper: {}", e);
            match e {
                errors::Error::ErrorWithRequeue(error) => return Ok(Action::requeue(error.duration)),
                other => return Ok(Action::await_change()),
            }
        }
    }

    // then reconcile pageserver
    match pageserver::reconcile(neon_storage.clone(), ctx.clone()).await {
        Ok(_) => (),
        Err(e) => {
            error!("failed to reconcile safekeeper: {}", e);
            match e {
                errors::Error::ErrorWithRequeue(error) => return Ok(Action::requeue(error.duration)),
                other => return Ok(Action::await_change()),
            }
        }
    }

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
    fn recorder(&self, client: Client, neon_storage: &NeonStorage) -> Recorder {
        Recorder::new(client, self.reporter.clone(), neon_storage.object_ref(&()))
    }
}

fn error_policy(neon_storage: Arc<NeonStorage>, error: &errors::Error, ctx: Arc<Context>) -> Action {
    warn!("reconcile failed: {:?}", error);
    ctx.metrics.reconcile_failure(&neon_storage, error);
    Action::requeue(Duration::from_secs(5 * 60))
}

/// Initialize the controller and shared state (given the crd is installed)
pub async fn run(state: State) {
    let client = Client::try_default().await.expect("failed to create kube Client");

    let neonstorages = Api::<NeonStorage>::all(client.clone());
    if let Err(e) = neonstorages.list(&ListParams::default().limit(1)).await {
        error!("CRD is not queryable; {e:?}. Is the CRD installed?");
        info!("Installation: cargo run --bin crdgen | kubectl apply -f -");
        std::process::exit(1);
    }

    Controller::new(neonstorages, Config::default().any_semantic())
        .shutdown_on_signal()
        .run(reconcile, error_policy, state.to_context(client))
        .filter_map(|x| async move { std::result::Result::ok(x) })
        .for_each(|_| futures::future::ready(()))
        .await;
}
