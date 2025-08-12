use serde::{Deserialize, Serialize};

use crate::api::v1::NodeId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantShardInfo {
    pub tenant_shard_id: String,
    pub node_attached: NodeId,
    #[serde(default)]
    pub node_secondary: Vec<NodeId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default)]
    pub is_reconciling: bool,
    #[serde(default)]
    pub is_pending_compute_notification: bool,
    #[serde(default)]
    pub is_splitting: bool,
    #[serde(default)]
    pub is_importing: bool,
    pub scheduling_policy: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_az_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantShardResponse {
    pub tenant_id: String,
    pub shards: Vec<TenantShardInfo>,
    pub stripe_size: u32,
    pub policy: serde_json::Value,
    pub config: serde_json::Value,
}
