use super::resources::*;
use crate::controllers::{pageserver, safekeeper, storage_broker, storage_controller};
use crate::util::cluster_status::{ClusterPhase, ClusterStatusManager};
use crate::util::{errors, errors::Result, metrics};
use chrono::{DateTime, Utc};
use futures::StreamExt;
use k8s_openapi::api::{
    apps::v1::{Deployment, StatefulSet},
    core::v1::{PersistentVolumeClaim, Pod, Service},
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
use serde::Serialize;
use serde_json::json;
use std::sync::Arc;
use tokio::{sync::RwLock, time::Duration};
use tracing::*;

// Define a constant at the top of the file
pub const FIELD_MANAGER: &str = "neon-cluster-controller";

impl NeonCluster {
    // Reconcile (for non-finalizer related changes)
    pub async fn reconcile(&self, ctx: Arc<Context>) -> Result<Action, errors::Error> {
        let client = ctx.client.clone();
        let _ = ctx.diagnostics.read().await.recorder(client.clone());

        // Initialize status manager
        let status_manager = ClusterStatusManager::new(&client, self)?;

        // if status is not set, set it to default
        if self.status.is_none() {
            let cluster_client: Api<NeonCluster> =
                Api::namespaced(client.clone(), &self.namespace().unwrap());
            let new_status = Patch::Apply(json!({
                "apiVersion": "oltp.molnett.org/v1",
                "kind": "NeonCluster",
                "status": NeonClusterStatus {
                    conditions: Vec::new(),
                    phase: Some(ClusterPhase::Pending.to_string()),
                    page_server_status: NeonClusterPageServerStatus::default(),
                    storage_broker_status: NeonClusterStorageBrokerStatus::default(),
                    safekeeper_status: NeonClusterSafeKeeperStatus::default(),
                }
            }));

            let ps = PatchParams::apply(FIELD_MANAGER).force();
            let _o = cluster_client
                .patch_status(&self.name_any(), &ps, &new_status)
                .await
                .map_err(|e| errors::Error::StdError(errors::StdError::KubeError(e)))?;

            // Set initial phase
            status_manager.update_phase(ClusterPhase::Creating).await?;
        }

        // Ensure JWT signing keys exist
        if let Err(e) = self.ensure_jwt_keys(&ctx.client).await {
            error!("failed to ensure JWT keys: {}", e);
            match e {
                errors::Error::ErrorWithRequeue(error) => return Ok(Action::requeue(error.duration)),
                _ => return Err(e),
            }
        }

        // Storage controller
        match storage_controller::reconcile(self, ctx.clone()).await {
            Ok(_) => (),
            Err(e) => {
                error!("failed to reconcile storage controller: {}", e);
                match e {
                    errors::Error::ErrorWithRequeue(error) => return Ok(Action::requeue(error.duration)),
                    _ => return Err(e),
                }
            }
        }

        // Storage broker
        match storage_broker::reconcile(self, ctx.clone()).await {
            Ok(_) => (),
            Err(e) => {
                error!("failed to reconcile storage broker: {}", e);
                match e {
                    errors::Error::ErrorWithRequeue(error) => return Ok(Action::requeue(error.duration)),
                    _ => return Err(e),
                }
            }
        }

        // then reconcile safekeeper
        match safekeeper::reconcile(self, ctx.clone()).await {
            Ok(_) => (),
            Err(e) => {
                error!("failed to reconcile safekeeper: {}", e);
                match e {
                    errors::Error::ErrorWithRequeue(error) => return Ok(Action::requeue(error.duration)),
                    _ => return Err(e),
                }
            }
        }

        // then reconcile pageserver
        match pageserver::reconcile(self, ctx.clone()).await {
            Ok(_) => (),
            Err(e) => {
                error!("failed to reconcile safekeeper: {}", e);
                match e {
                    errors::Error::ErrorWithRequeue(error) => return Ok(Action::requeue(error.duration)),
                    _ => return Err(e),
                }
            }
        }

        // If no events were received, check back every 5 minutes
        Ok(Action::requeue(Duration::from_secs(5 * 60)))
    }

    // Finalizer cleanup (the object was deleted, ensure nothing is orphaned)
    async fn cleanup(&self, ctx: Arc<Context>) -> Result<Action> {
        let recorder = ctx.diagnostics.read().await.recorder(ctx.client.clone());
        // Cluster doesn't have any real cleanup, so we just publish an event
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

    async fn ensure_jwt_keys(&self, client: &Client) -> Result<()> {
        use crate::util::jwt_keys::Ed25519KeyPair;
        use k8s_openapi::api::core::v1::Secret;
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

        let namespace = self.namespace().unwrap();
        let secret_name = format!("{}-jwt-keys", self.name_any());
        let secrets: Api<Secret> = Api::namespaced(client.clone(), &namespace);

        // Check if secret already exists
        match secrets.get(&secret_name).await {
            Ok(_) => {
                info!("JWT keys secret '{}' already exists", secret_name);
                return Ok(());
            }
            Err(kube::Error::Api(err)) if err.code == 404 => {
                // Secret doesn't exist, create it
                info!("Generating new JWT keys for cluster '{}'", self.name_any());
            }
            Err(e) => return Err(errors::Error::StdError(errors::StdError::KubeError(e))),
        }

        // Generate new key pair
        let keypair = Ed25519KeyPair::generate()?;
        let secret_data = keypair.to_secret_data()?;

        // Create the secret
        let secret = Secret {
            metadata: ObjectMeta {
                name: Some(secret_name.clone()),
                namespace: Some(namespace.clone()),
                owner_references: self.controller_owner_ref(&()).map(|owner_ref| vec![owner_ref]),
                labels: Some(
                    [
                        ("app.kubernetes.io/name".to_string(), "neon-operator".to_string()),
                        ("app.kubernetes.io/component".to_string(), "jwt-keys".to_string()),
                        ("neon.cluster.name".to_string(), self.name_any()),
                    ]
                    .into_iter()
                    .collect(),
                ),
                ..Default::default()
            },
            data: Some(secret_data),
            ..Default::default()
        };

        secrets
            .create(&Default::default(), &secret)
            .await
            .map_err(|e| errors::Error::StdError(errors::StdError::KubeError(e)))?;

        info!("Created JWT keys secret: {}", secret_name);
        Ok(())
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

pub async fn reconcile(neon_cluster: Arc<NeonCluster>, ctx: Arc<Context>) -> Result<Action> {
    let _timer = ctx.metrics.count_and_measure("cluster");
    ctx.diagnostics.write().await.last_event = Utc::now();

    let ns = neon_cluster.namespace().unwrap(); // neon_cluster is namespace scoped
    let neon_clusters: Api<NeonCluster> = Api::namespaced(ctx.client.clone(), &ns);

    info!(
        "Reconciling neon_clusterument \"{}\" in {}",
        neon_cluster.name_any(),
        ns
    );
    finalizer(
        &neon_clusters,
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
    fn recorder(&self, client: Client) -> Recorder {
        Recorder::new(client, self.reporter.clone())
    }
}

fn error_policy(neon_cluster: Arc<NeonCluster>, error: &errors::Error, ctx: Arc<Context>) -> Action {
    warn!("reconcile failed: {:?}", error);
    ctx.metrics.reconcile_cluster_failure(&neon_cluster, error);
    Action::requeue(Duration::from_secs(5 * 60))
}

/// Initialize the controller and shared state (given the crd is installed)
pub async fn run(state: State) {
    let client = Client::try_default().await.expect("failed to create kube Client");

    let neonclusters = Api::<NeonCluster>::all(client.clone());
    if let Err(e) = neonclusters.list(&ListParams::default().limit(1)).await {
        error!("CRD is not queryable; {e:?}. Is the CRD installe?");
        info!("Installation: cargo run --bin crdgen | kubectl apply -f -");
        std::process::exit(1);
    }

    Controller::new(neonclusters, Config::default().any_semantic())
        .owns(
            Api::<StatefulSet>::all(client.clone()),
            watcher::Config::default(),
        )
        .owns(
            Api::<Pod>::all(client.clone()),
            watcher::Config::default().labels("app.kubernetes.io/component=pageserver"),
        )
        .owns(
            Api::<PersistentVolumeClaim>::all(client.clone()),
            watcher::Config::default().labels("app.kubernetes.io/component=pageserver"),
        )
        .owns(Api::<Service>::all(client.clone()), watcher::Config::default())
        .owns(Api::<Deployment>::all(client.clone()), watcher::Config::default())
        .run(reconcile, error_policy, state.to_context(client))
        .filter_map(|x| async move { std::result::Result::ok(x) })
        .for_each(|_| futures::future::ready(()))
        .await;
}
