use crate::neon_cluster::controller::{Context, NeonCluster};
use crate::util::errors::{Error, ErrorWithRequeue, StdError};
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
use std::sync::Arc;
use tokio::time::Duration;

pub fn reconcile(neon_cluster: Arc<NeonCluster>, ctx: Arc<Context>) -> Result<Action, Error> {
    Ok(Action::requeue(Duration::from_secs(5 * 60)))
}
