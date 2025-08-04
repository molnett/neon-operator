use k8s_openapi::serde_json;
use kube::Client;
use neon_cluster::compute::spec::generate_compute_spec;

/// Service for compute-related operations
pub struct ComputeService {
    client: Client,
}

impl ComputeService {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    /// Generate compute spec for a given compute ID
    pub async fn generate_spec(&self, compute_id: &str) -> Result<serde_json::Value, String> {
        generate_compute_spec(&self.client, None, compute_id)
            .await
            .map_err(|e| e.to_string())
    }
}
