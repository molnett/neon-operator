use crate::controllers::resources::NeonCluster;
use crate::util::errors::{Error, Result, StdError};
use crate::util::jwt_keys::{self, Ed25519KeyPair};
use k8s_openapi::api::core::v1::Secret;
use kube::{api::Api, ResourceExt};

pub async fn get_jwt_keys_from_secret(
    client: &kube::Client,
    cluster_name: &str,
) -> Result<serde_json::Value> {
    // First, find the NeonCluster resource to get its namespace
    let clusters: Api<NeonCluster> = Api::all(client.clone());
    let cluster = clusters
        .list(&kube::api::ListParams::default())
        .await
        .map_err(|e| Error::StdError(StdError::KubeError(e)))?
        .items
        .into_iter()
        .find(|c| c.name_any() == cluster_name)
        .ok_or_else(|| Error::StdError(StdError::MetadataMissing("NeonCluster not found".to_string())))?;

    let cluster_namespace = cluster.namespace().unwrap();
    let secrets: Api<Secret> = Api::namespaced(client.clone(), &cluster_namespace);
    let secret_name = format!("{}-jwt-keys", cluster_name);

    let secret = secrets
        .get(&secret_name)
        .await
        .map_err(|e| Error::StdError(StdError::KubeError(e)))?;

    let secret_data = secret
        .data
        .as_ref()
        .ok_or_else(|| Error::StdError(StdError::MetadataMissing("Secret has no data".to_string())))?;

    let jwks_data = secret_data
        .get("jwks")
        .ok_or_else(|| Error::StdError(StdError::MetadataMissing("jwks not found in secret".to_string())))?;

    let jwks_str = String::from_utf8(jwks_data.0.clone())
        .map_err(|_| Error::StdError(StdError::MetadataMissing("Invalid UTF-8 in jwks".to_string())))?;

    serde_json::from_str(&jwks_str).map_err(|e| Error::StdError(StdError::JsonSerializationError(e)))
}

pub async fn get_key_pair_from_secret(client: &kube::Client, cluster_name: &str) -> Result<Ed25519KeyPair> {
    // First, find the NeonCluster resource to get its namespace
    let clusters: Api<NeonCluster> = Api::all(client.clone());
    let cluster = clusters
        .list(&kube::api::ListParams::default())
        .await
        .map_err(|e| Error::StdError(StdError::KubeError(e)))?
        .items
        .into_iter()
        .find(|c| c.name_any() == cluster_name)
        .ok_or_else(|| Error::StdError(StdError::MetadataMissing("NeonCluster not found".to_string())))?;

    let cluster_namespace = cluster.namespace().unwrap();
    let secrets: Api<Secret> = Api::namespaced(client.clone(), &cluster_namespace);
    let secret_name = format!("{}-jwt-keys", cluster_name);

    let secret = secrets
        .get(&secret_name)
        .await
        .map_err(|e| Error::StdError(StdError::KubeError(e)))?;

    let secret_data = secret
        .data
        .as_ref()
        .ok_or_else(|| Error::StdError(StdError::MetadataMissing("Secret has no data".to_string())))?;

    Ok(Ed25519KeyPair::from_secret_data(secret_data)?)
}
