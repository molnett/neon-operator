use crate::util::errors::{Error, Result, StdError};

use k8s_openapi::api::core::v1::ConfigMap;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};

use kube::{
    api::{Patch, PatchParams, PostParams},
    Api, Client,
};

use std::collections::BTreeMap;
use tracing::info;

pub async fn reconcile_configmap(
    client: &Client,
    namespace: &str,
    name: &str,
    cluster_name: &str,
    bucket_credentials_secret: &str,
    oref: &OwnerReference,
) -> Result<()> {
    let configmaps: Api<ConfigMap> = Api::namespaced(client.clone(), namespace);
    let desired_configmap = create_desired_configmap(
        namespace,
        name,
        cluster_name,
        bucket_credentials_secret,
        client,
        oref,
    )
    .await?;

    match configmaps.get(name).await {
        Ok(existing) => {
            if configmap_needs_update(&existing, &desired_configmap) {
                info!("Updating ConfigMap '{}'", name);
                configmaps
                    .patch(
                        name,
                        &PatchParams::apply("kube-rs-controller").force(),
                        &Patch::Apply(&desired_configmap),
                    )
                    .await
                    .map_err(|e| Error::StdError(StdError::KubeError(e)))?;
            } else {
                info!("ConfigMap '{}' is up to date", name);
            }
        }
        Err(kube::Error::Api(api_err)) if api_err.code == 404 => {
            info!("Creating ConfigMap '{}'", name);
            configmaps
                .create(&PostParams::default(), &desired_configmap)
                .await
                .map_err(|e| Error::StdError(StdError::KubeError(e)))?;
        }
        Err(e) => return Err(Error::StdError(StdError::KubeError(e))),
    }

    Ok(())
}

async fn create_desired_configmap(
    namespace: &str,
    name: &str,
    cluster_name: &str,
    bucket_credentials_secret: &str,
    client: &Client,
    oref: &OwnerReference,
) -> Result<ConfigMap> {
    // Fetch bucket credentials from the secret
    let secrets: Api<k8s_openapi::api::core::v1::Secret> = Api::namespaced(client.clone(), namespace);
    let secret = secrets
        .get(bucket_credentials_secret)
        .await
        .map_err(|e| Error::StdError(StdError::KubeError(e)))?;

    let secret_data = secret
        .data
        .as_ref()
        .ok_or_else(|| Error::StdError(StdError::MetadataMissing("Secret has no data".to_string())))?;

    let bucket_name = String::from_utf8(
        secret_data
            .get("BUCKET_NAME")
            .ok_or_else(|| {
                Error::StdError(StdError::MetadataMissing(
                    "BUCKET_NAME not found in secret".to_string(),
                ))
            })?
            .0
            .clone(),
    )
    .map_err(|_| {
        Error::StdError(StdError::MetadataMissing(
            "Invalid UTF-8 in BUCKET_NAME".to_string(),
        ))
    })?;

    let aws_region = String::from_utf8(
        secret_data
            .get("AWS_REGION")
            .ok_or_else(|| {
                Error::StdError(StdError::MetadataMissing(
                    "AWS_REGION not found in secret".to_string(),
                ))
            })?
            .0
            .clone(),
    )
    .map_err(|_| {
        Error::StdError(StdError::MetadataMissing(
            "Invalid UTF-8 in AWS_REGION".to_string(),
        ))
    })?;

    let aws_endpoint_url = String::from_utf8(
        secret_data
            .get("AWS_ENDPOINT_URL")
            .ok_or_else(|| {
                Error::StdError(StdError::MetadataMissing(
                    "AWS_ENDPOINT_URL not found in secret".to_string(),
                ))
            })?
            .0
            .clone(),
    )
    .map_err(|_| {
        Error::StdError(StdError::MetadataMissing(
            "Invalid UTF-8 in AWS_ENDPOINT_URL".to_string(),
        ))
    })?;

    Ok(ConfigMap {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(namespace.to_string()),
            owner_references: Some(vec![oref.clone()]),
            ..Default::default()
        },
        data: Some({
            let mut data = BTreeMap::new();
            data.insert(
                "pageserver.toml".to_string(),
                format!(
                    r#"
                        control_plane_api = "http://storage-controller-{0}:8080/upcall/v1/"
                        listen_pg_addr = "0.0.0.0:6400"
                        listen_http_addr = "0.0.0.0:9898"
                        broker_endpoint = "http://storage-broker-{}:50051"
                        pg_distrib_dir='/usr/local/'
                        [remote_storage]
                        bucket_name = "{}"
                        bucket_region = "{}"
                        prefix_in_bucket = "pageserver"
                        endpoint = "{}"
                    "#,
                    cluster_name, bucket_name, aws_region, aws_endpoint_url
                )
                .to_string(),
            );
            // Identity.toml will be created by init container
            // data.insert("identity.toml".to_string(), "id=DYNAMIC".to_string());
            // Metadata.json will be created by init container
            // data.insert("metadata.json".to_string(), "DYNAMIC".to_string());
            data
        }),
        ..Default::default()
    })
}

pub fn configmap_needs_update(existing: &ConfigMap, desired: &ConfigMap) -> bool {
    existing.data != desired.data
}
