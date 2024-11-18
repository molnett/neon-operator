#[cfg(test)]
mod tests {
    use crate::controllers::cluster_controller::{reconcile, State};
    use crate::controllers::resources::NeonCluster;
    use crate::controllers::resources::{NeonClusterSpec, PGVersion};
    use crate::util::telemetry;
    use k8s_openapi::api::apps::v1::Deployment;
    use kube::api::{Api, ListParams, ObjectMeta, Patch, PatchParams};
    use kube::Client;
    use std::sync::Arc;

    #[tokio::test]
    #[ignore = "uses k8s current-context"]
    async fn integration_reconcile_should_set_status() {
        let client = Client::try_default().await.unwrap();
        let ctx = State::default().to_context(client.clone());

        // Create a test NeonCluster
        let neon_cluster = NeonCluster {
            metadata: ObjectMeta {
                name: Some("test-cluster".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            spec: NeonClusterSpec {
                num_safekeepers: 3,
                default_pg_version: PGVersion::PG15,
                neon_image: "neondatabase/neon:latest".to_string(),
                bucket_credentials_secret: "neon-bucket-credentials".to_string(),
            },
            status: None,
        };

        let clusters: Api<NeonCluster> = Api::namespaced(client.clone(), "default");
        let ssapply = PatchParams::apply("ctrltest").force();
        let patch = Patch::Apply(&neon_cluster);
        clusters.patch("test-cluster", &ssapply, &patch).await.unwrap();

        // Reconcile the NeonCluster
        neon_cluster.reconcile(ctx).await.unwrap();

        // Verify that the status has been updated
        let output = clusters.get("test-cluster").await.unwrap();
        assert!(output.status.is_some());

        // Check that pageserver, storage broker and safekeepers are created
        let deployment_client: Api<Deployment> = Api::namespaced(client.clone(), "default");
        let pageserver_deployment = deployment_client.get("pageserver-test-cluster").await.unwrap();

        assert_eq!(pageserver_deployment.status.unwrap().ready_replicas, Some(1));
    }

    async fn delete_cluster(name: &str) {
        let client = Client::try_default().await.unwrap();
        let ctx = State::default().to_context(client.clone());
        let clusters: Api<NeonCluster> = Api::namespaced(client.clone(), "default");
        clusters.delete(name, &Default::default()).await.unwrap();
    }
}
