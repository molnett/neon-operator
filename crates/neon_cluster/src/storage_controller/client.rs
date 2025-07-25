use super::types::TenantShardResponse;
use crate::util::errors::{Error, Result, StdError};
use tracing::{error, info};

pub struct StorageControllerClient {
    base_url: String,
    client: reqwest::Client,
}

impl StorageControllerClient {
    pub fn new(cluster_name: &str) -> Self {
        Self {
            base_url: format!("http://storage-controller-{}:8080", cluster_name),
            client: reqwest::Client::new(),
        }
    }

    pub async fn get_pageserver_connstring(&self, tenant_id: &str) -> Result<String> {
        let url = format!("{}/control/v1/tenant/{}", self.base_url, tenant_id);
        info!("Fetching tenant info from storage controller: {}", url);

        let response = self.client.get(&url).send().await.map_err(|e| {
            error!("Failed to connect to storage controller: {}", e);
            Error::StdError(StdError::HttpError(format!(
                "Storage controller request failed: {}",
                e
            )))
        })?;

        if !response.status().is_success() {
            error!("Storage controller returned error status: {}", response.status());
            return Err(Error::StdError(StdError::HttpError(format!(
                "Storage controller returned {}",
                response.status()
            ))));
        }

        let tenant_info: TenantShardResponse = response.json().await.map_err(|e| {
            error!("Failed to parse JSON response: {}", e);
            Error::StdError(StdError::SerializationError(format!(
                "Failed to parse tenant info JSON: {}",
                e
            )))
        })?;

        // Find the shard with matching tenant_shard_id
        let primary_shard = tenant_info
            .shards
            .iter()
            .find(|s| s.tenant_shard_id == tenant_id)
            .ok_or_else(|| {
                error!("Primary shard not found for tenant {}", tenant_id);
                Error::StdError(StdError::MetadataMissing(format!(
                    "Primary shard not found for tenant {}",
                    tenant_id
                )))
            })?;

        let connstring = format!(
            "host=pageserver-{}.pageserver-basic-cluster-headless.neon.svc.cluster.local port=6400",
            primary_shard.node_attached
        );

        info!("Retrieved pageserver connection string: {}", connstring);
        Ok(connstring)
    }

    pub async fn get_tenant_info(&self, tenant_id: &str) -> Result<TenantShardResponse> {
        let url = format!("{}/control/v1/tenant/{}", self.base_url, tenant_id);
        info!("Fetching full tenant info from storage controller: {}", url);

        let response = self.client.get(&url).send().await.map_err(|e| {
            error!("Failed to connect to storage controller: {}", e);
            Error::StdError(StdError::HttpError(format!(
                "Storage controller request failed: {}",
                e
            )))
        })?;

        if !response.status().is_success() {
            error!("Storage controller returned error status: {}", response.status());
            return Err(Error::StdError(StdError::HttpError(format!(
                "Storage controller returned {}",
                response.status()
            ))));
        }

        Ok(response.json::<TenantShardResponse>().await.unwrap())
    }
}
