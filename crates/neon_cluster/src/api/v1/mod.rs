use core::fmt;
use std::fmt::Display;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub mod neonbranch;
pub mod neoncluster;
pub mod neonproject;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct NodeId(pub u64);

impl Default for NodeId {
    fn default() -> Self {
        NodeId(0)
    }
}

impl Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Default, Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub enum PGVersion {
    PG14 = 14,
    PG15 = 15,
    PG16 = 16,
    #[default]
    PG17 = 17,
}

impl Display for PGVersion {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.clone() as isize)
    }
}

pub fn conditions_schema(_: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
    serde_json::from_value(serde_json::json!({
        "type": "array",
        "x-kubernetes-list-type": "map",
        "x-kubernetes-list-map-keys": ["type"],
        "items": {
            "type": "object",
            "properties": {
                "lastTransitionTime": { "format": "date-time", "type": "string" },
                "message": { "type": "string" },
                "observedGeneration": { "type": "integer", "format": "int64", "default": 0 },
                "reason": { "type": "string" },
                "status": { "type": "string" },
                "type": { "type": "string" }
            },
            "required": [
                "lastTransitionTime",
                "message",
                "reason",
                "status",
                "type"
            ],
        },
    }))
    .unwrap()
}
