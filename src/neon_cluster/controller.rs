use crate::neon_cluster::storage_broker;
use chrono::{DateTime, Utc};
use futures::StreamExt;
use k8s_openapi::api::{
    apps::v1::{Deployment, StatefulSet},
    core::v1::{ConfigMap, Service},
};
use kube::{
    api::{Api, ListParams, Patch, PatchParams, PostParams, Request, ResourceExt},
    client::Client,
    runtime::{
        controller::{Action, Controller},
        events::{Event, EventType, Recorder, Reporter},
        finalizer::{finalizer, Event as Finalizer},
        watcher::{self, Config},
    },
    CustomResource, Resource,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tokio::{sync::RwLock, time::Duration};
use tracing::*;

use crate::util::{errors, errors::Result, metrics, telemetry};

use super::resources::{
    pageserver_configmap, pageserver_service, pageserver_statefulset, safekeeper_service,
    safekeeper_statefulset, storage_broker_deployment, storage_broker_service,
};

pub static NEON_CLUSTER_FINALIZER: &str = "neon-cluster.oltp.molnett.org";

#[derive(Default, Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub enum PGVersion {
    PG14 = 14,
    #[default]
    PG15 = 15,
    PG16 = 16,
}
/// Generate the Kubernetes wrapper struct `NeonCluster` from our Spec and Status struct
///
/// This provides a hook for generating the CRD yaml (in crdgen.rs)
#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[cfg_attr(test, derive(Default))]
#[kube(kind = "NeonCluster", group = "oltp.molnett.org", version = "v1", namespaced)]
#[kube(status = "NeonClusterStatus", shortname = "neoncluster")]
pub struct NeonClusterSpec {
    #[serde(default = "default_num_safekeepers")]
    pub num_safekeepers: u8,
    #[serde(default = "default_pg_version")]
    pub default_pg_version: PGVersion,
    #[serde(default = "default_neon_image")]
    pub neon_image: String,
    pub rook_bucket_object: Option<String>,
    pub bucket_credentials_secret: Option<String>,
}

fn default_num_safekeepers() -> u8 {
    3
}
fn default_pg_version() -> PGVersion {
    PGVersion::PG15
}
fn default_neon_image() -> String {
    "neondatabase/neon:latest".to_string()
}

/// The status object of `NeonCluster`
#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct NeonClusterStatus {
    pub page_server_status: NeonClusterPageServerStatus,
    pub storage_broker_status: NeonClusterStorageBrokerStatus,
    pub safekeeper_status: NeonClusterSafeKeeperStatus,
}

/// The status object of `NeonCluster`
#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct NeonClusterPageServerStatus {}

/// The status object of `NeonCluster`
#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct NeonClusterStorageBrokerStatus {}

/// The status object of `NeonCluster`
#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct NeonClusterSafeKeeperStatus {}

impl NeonCluster {
    // Reconcile (for non-finalizer related changes)
    async fn reconcile(&self, ctx: Arc<Context>) -> Result<Action, errors::Error> {
        let client = ctx.client.clone();
        let recorder = ctx.diagnostics.read().await.recorder(client.clone(), self);
        let ns = self.namespace().unwrap();
        let name = self.name_any();
        let cluster_object: Api<NeonCluster> = Api::namespaced(client.clone(), &ns);

        if self.spec.bucket_credentials_secret.is_none() && self.spec.rook_bucket_object.is_none() {
            return Err(errors::Error::ErrorWithRequeue(errors::ErrorWithRequeue {
                duration: Duration::from_secs(60),
                error: errors::StdError::MissingBucketConfig,
            }));
        }
        if self.spec.bucket_credentials_secret.is_some() && self.spec.rook_bucket_object.is_some() {
            return Err(errors::Error::ErrorWithRequeue(errors::ErrorWithRequeue {
                duration: Duration::from_secs(60),
                error: errors::StdError::ConflictingBucketConfig(
                    "Cannot set both BucketCredentialsSecret and RookBucketObject".to_string(),
                ),
            }));
        }

        let oref = self.controller_owner_ref(&()).unwrap();

        let statefulset_client: Api<StatefulSet> = Api::namespaced(client.clone(), &ns);
        let service_client: Api<Service> = Api::namespaced(client.clone(), &ns);
        let deployment_client: Api<Deployment> = Api::namespaced(client.clone(), &ns);
        let configmap_client: Api<ConfigMap> = Api::namespaced(client.clone(), &ns);

        let safekeeper_ss_object = statefulset_client
            .get_opt(&format!("{}-safekeeper", name))
            .await
            .map_err(|e| errors::Error::StdError(errors::StdError::KubeError(e)))?;

        // If safekeeper statefulset is not found, create it
        if safekeeper_ss_object.is_none() {
            statefulset_client
                .create(
                    &PostParams::default(),
                    &safekeeper_statefulset(self, oref.clone()),
                )
                .await
                .map_err(|e| errors::Error::StdError(errors::StdError::KubeError(e)))?;
        }

        // Check if safekeeper service exists
        let safekeeper_service_object = service_client
            .get_opt(&format!("{}-safekeeper", name))
            .await
            .map_err(|e| errors::Error::StdError(errors::StdError::KubeError(e)))?;

        // If safekeeper service is not found, create it
        if safekeeper_service_object.is_none() {
            service_client
                .create(&PostParams::default(), &safekeeper_service(self, oref.clone()))
                .await
                .map_err(|e| errors::Error::StdError(errors::StdError::KubeError(e)))?;
        }

        // Storage broker
        let storage_broker_deployment_object = deployment_client
            .get_opt(&format!("{}-storage-broker", name))
            .await
            .map_err(|e| errors::Error::StdError(errors::StdError::KubeError(e)))?;

        // If storage broker deployment is not found, create it
        if storage_broker_deployment_object.is_none() {
            deployment_client
                .create(
                    &PostParams::default(),
                    &storage_broker_deployment(self, oref.clone()),
                )
                .await
                .map_err(|e| errors::Error::StdError(errors::StdError::KubeError(e)))?;
        }

        // Storage broker service
        let storage_broker_service_object = service_client
            .get_opt(&format!("{}-storage-broker", name))
            .await
            .map_err(|e| errors::Error::StdError(errors::StdError::KubeError(e)))?;

        // If storage broker service is not found, create it
        if storage_broker_service_object.is_none() {
            service_client
                .create(
                    &PostParams::default(),
                    &storage_broker_service(self, oref.clone()),
                )
                .await
                .map_err(|e| errors::Error::StdError(errors::StdError::KubeError(e)))?;
        }

        // Pageserver
        let pageserver_ss_object = statefulset_client
            .get_opt(&format!("{}-pageserver", name))
            .await
            .map_err(|e| errors::Error::StdError(errors::StdError::KubeError(e)))?;

        // If pageserver deployment is not found, create it
        if pageserver_ss_object.is_none() {
            statefulset_client
                .create(
                    &PostParams::default(),
                    &pageserver_statefulset(self, oref.clone()),
                )
                .await
                .map_err(|e| errors::Error::StdError(errors::StdError::KubeError(e)))?;
        }

        // Service
        let service_object = service_client
            .get_opt(&format!("{}-pageserver", name))
            .await
            .map_err(|e| errors::Error::StdError(errors::StdError::KubeError(e)))?;

        // If service is not found, create it
        if service_object.is_none() {
            service_client
                .create(&PostParams::default(), &pageserver_service(self, oref.clone()))
                .await
                .map_err(|e| errors::Error::StdError(errors::StdError::KubeError(e)))?;
        }

        // Configmap
        let configmap_object = configmap_client
            .get_opt(&format!("{}-pageserver-config", name))
            .await
            .map_err(|e| errors::Error::StdError(errors::StdError::KubeError(e)))?;

        // If configmap is not found, create it
        if configmap_object.is_none() {
            configmap_client
                .create(&PostParams::default(), &pageserver_configmap(self, oref.clone()))
                .await
                .map_err(|e| errors::Error::StdError(errors::StdError::KubeError(e)))?;
        }

        // always overwrite status object with what we saw
        let new_status = Patch::Apply(json!({
            "apiVersion": "oltp.molnett.org/v1",
            "kind": "NeonCluster",
            "status": NeonClusterStatus {
                page_server_status: NeonClusterPageServerStatus{},
                storage_broker_status: NeonClusterStorageBrokerStatus{},
                safekeeper_status: NeonClusterSafeKeeperStatus{},
            }
        }));
        let ps = PatchParams::apply("cntrlr").force();
        let _o = cluster_object
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

#[instrument(skip(ctx, neon_cluster), fields(trace_id))]
pub async fn reconcile(neon_cluster: Arc<NeonCluster>, ctx: Arc<Context>) -> Result<Action> {
    let trace_id = telemetry::get_trace_id();
    Span::current().record("trace_id", field::display(&trace_id));
    let _timer = ctx.metrics.count_and_measure();
    ctx.diagnostics.write().await.last_event = Utc::now();

    let ns = neon_cluster.namespace().unwrap(); // neon_cluster is namespace scoped
    let cluster_client: Api<NeonCluster> = Api::namespaced(ctx.client.clone(), &ns);

    info!(
        "Reconciling neon_cluster \"{}\" in {}",
        neon_cluster.name_any(),
        ns
    );

    finalizer(
        &cluster_client,
        NEON_CLUSTER_FINALIZER,
        neon_cluster.clone(),
        |event| async {
            match event {
                Finalizer::Apply(neon_cluster) => neon_cluster.reconcile(ctx.clone()).await,
                Finalizer::Cleanup(neon_cluster) => neon_cluster.cleanup(ctx.clone()).await,
            }
        },
    )
    .await
    .map_err(|e| errors::Error::StdError(errors::StdError::FinalizerError(Box::new(e))))?;

    // first reconcile storage broker
    match storage_broker::reconcile(neon_cluster.clone(), ctx.clone()) {
        Ok(action) => return Ok(action),
        Err(e) => {
            error!("failed to reconcile storage broker: {}", e);
            if let errors::Error::ErrorWithRequeue(error) = e {
                return Ok(Action::requeue(error.duration));
            }
        }
    }

    Ok(Action::requeue(Duration::from_secs(60)))
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
    fn recorder(&self, client: Client, neon_cluster: &NeonCluster) -> Recorder {
        Recorder::new(client, self.reporter.clone(), neon_cluster.object_ref(&()))
    }
}

fn error_policy(neon_cluster: Arc<NeonCluster>, error: &errors::Error, ctx: Arc<Context>) -> Action {
    warn!("reconcile failed: {:?}", error);
    ctx.metrics.reconcile_failure(&neon_cluster, error);
    Action::requeue(Duration::from_secs(5 * 60))
}

/// Initialize the controller and shared state (given the crd is installed)
pub async fn run(state: State) {
    let client = Client::try_default().await.expect("failed to create kube Client");

    let neon_clusters = Api::<NeonCluster>::all(client.clone());
    if let Err(e) = neon_clusters.list(&ListParams::default().limit(1)).await {
        error!("CRD is not queryable; {e:?}. Is the CRD installed?");
        info!("Installation: cargo run --bin crdgen | kubectl apply -f -");
        std::process::exit(1);
    }

    Controller::new(neon_clusters, Config::default().any_semantic())
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
