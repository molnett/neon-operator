use crate::controller::Diagnostics;
use crate::storage_broker::*;
use crate::{telemetry, Error, Metrics, Result};
use chrono::{DateTime, Utc};
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

/// Generate the Kubernetes wrapper struct `NeonStorage` from our Spec and Status struct
///
/// This provides a hook for generating the CRD yaml (in crdgen.rs)
#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[cfg_attr(test, derive(Default))]
#[kube(kind = "NeonStorage", group = "kube.rs", version = "v1", namespaced)]
#[kube(status = "NeonStorageStatus", shortname = "doc")]
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

// Context for our reconciler
#[derive(Clone)]
pub struct Context {
    /// Kubernetes client
    pub client: Client,
    /// Diagnostics read by the web server
    pub diagnostics: Arc<RwLock<Diagnostics>>,
    /// Prometheus metrics
    pub metrics: Metrics,
}

#[instrument(skip(ctx, neon_storage), fields(trace_id))]
async fn reconcile(neon_storage: Arc<NeonStorage>, ctx: Arc<Context>) -> Result<Action> {
    let trace_id = telemetry::get_trace_id();
    Span::current().record("trace_id", &field::display(&trace_id));
    let _timer = ctx.metrics.count_and_measure();
    ctx.diagnostics.write().await.last_event = Utc::now();

    // first reconcile storage broker
    match crate::storage_broker::reconcile(neon_storage.clone(), ctx.clone()) {
        Ok(action) => return Ok(action),
        Err(e) => {
            error!("failed to reconcile storage broker: {}", e);
            match e {
                crate::Error::ErrorWithRequeue(error) => return Ok(Action::requeue(error.duration)),
                other => return Ok(Action::await_change()),
            }
        }
    }

    Ok(Action::requeue(Duration::from_secs(5 * 60)))
}
