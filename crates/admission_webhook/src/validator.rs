use anyhow::Result;
use kube::{
    api::{ListParams, TypeMeta},
    core::admission::{AdmissionRequest, AdmissionResponse, AdmissionReview, Operation},
    Api, Client,
};
use neon_cluster::api::v1alpha1::neonpageserver::NeonPageserver;
use tracing::{debug, warn};

#[derive(Clone)]
pub struct PageserverValidator {
    client: Client,
}

impl PageserverValidator {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    pub async fn validate_pageserver(
        &self,
        mut review: AdmissionReview<NeonPageserver>,
    ) -> Result<AdmissionReview<NeonPageserver>> {
        debug!("Processing admission review for NeonPageserver");
        let request = match review.request.take() {
            Some(req) => req,
            None => {
                return Ok(AdmissionReview {
                    response: Some(AdmissionResponse::invalid("Missing admission request")),
                    request: None,
                    types: TypeMeta {
                        api_version: "admission.k8s.io/v1".to_string(),
                        kind: "AdmissionReview".to_string(),
                    },
                });
            }
        };

        let mut response = match self.validate_request(&request).await {
            Ok(()) => {
                let mut resp = AdmissionResponse::invalid(""); // Will override
                resp.allowed = true;
                resp.result = Default::default();
                resp
            }
            Err(e) => {
                warn!("Validation failed: {}", e);
                AdmissionResponse::invalid(&e.to_string())
            }
        };

        response.uid = request.uid.clone();

        Ok(AdmissionReview {
            response: Some(response),
            request: Some(request),
            types: TypeMeta {
                api_version: "admission.k8s.io/v1".to_string(),
                kind: "AdmissionReview".to_string(),
            },
        })
    }

    async fn validate_request(&self, request: &AdmissionRequest<NeonPageserver>) -> Result<()> {
        // Only validate NeonPageserver resources
        if request.kind.kind != "NeonPageserver" {
            return Ok(());
        }

        match request.operation {
            Operation::Create => self.validate_create(request).await,
            Operation::Update => self.validate_update(request).await,
            _ => Ok(()), // Allow DELETE and other operations
        }
    }

    async fn validate_create(&self, request: &AdmissionRequest<NeonPageserver>) -> Result<()> {
        let pageserver = request
            .object
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing object in CREATE request"))?;

        debug!(
            "Validating CREATE for NeonPageserver with id={} cluster={}",
            pageserver.spec.id.0, pageserver.spec.cluster
        );

        // Check for ID uniqueness within the same cluster
        self.check_id_uniqueness(&pageserver.spec.id.0, &pageserver.spec.cluster, None)
            .await
    }

    async fn validate_update(&self, request: &AdmissionRequest<NeonPageserver>) -> Result<()> {
        let new_pageserver = request
            .object
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing object in UPDATE request"))?;
        let old_pageserver = request
            .old_object
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing old_object in UPDATE request"))?;

        debug!(
            "Validating UPDATE for NeonPageserver with id={} cluster={}",
            new_pageserver.spec.id.0, new_pageserver.spec.cluster
        );

        // Check immutability of ID, cluster, and storage_config
        if new_pageserver.spec.id != old_pageserver.spec.id {
            anyhow::bail!("Field 'id' is immutable and cannot be changed");
        }

        if new_pageserver.spec.cluster != old_pageserver.spec.cluster {
            anyhow::bail!("Field 'cluster' is immutable and cannot be changed");
        }

        if new_pageserver.spec.storage_config != old_pageserver.spec.storage_config {
            anyhow::bail!("Field 'storage_config' is immutable and cannot be changed");
        }

        // Since ID and cluster are immutable, no need to check uniqueness again
        // The object already exists and we're just updating other fields
        Ok(())
    }

    async fn check_id_uniqueness(
        &self,
        id: &u64,
        cluster: &str,
        exclude_namespace: Option<&str>,
    ) -> Result<()> {
        let api: Api<NeonPageserver> = Api::all(self.client.clone());
        let pageservers = api.list(&ListParams::default()).await?;

        for existing in pageservers.items {
            // Skip if it's the same object being updated
            if let Some(exclude_ns) = exclude_namespace {
                if let Some(existing_ns) = &existing.metadata.namespace {
                    if existing_ns == exclude_ns {
                        continue;
                    }
                }
            }

            // Check for conflict: same ID and same cluster
            if existing.spec.id.0 == *id && existing.spec.cluster == cluster {
                let existing_ns = existing
                    .metadata
                    .namespace
                    .unwrap_or_else(|| "default".to_string());
                anyhow::bail!(
                    "NeonPageserver with id={} already exists in cluster '{}' (namespace: {})",
                    id,
                    cluster,
                    existing_ns
                );
            }
        }

        Ok(())
    }
}
