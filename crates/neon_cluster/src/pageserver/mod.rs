use k8s_openapi::api::apps::v1::StatefulSet;

pub mod config;
pub mod deployment;
pub mod reconcile;
pub mod service;

// Re-export the main reconcile function
pub use reconcile::reconcile;

// Re-export utility functions that might be used elsewhere
pub use config::configmap_needs_update;
pub use service::service_needs_update;

// Keep statefulset_needs_update for backward compatibility, even though it's not used in the new structure
pub fn statefulset_needs_update(existing: &StatefulSet, desired: &StatefulSet) -> bool {
    let existing_spec = existing.spec.as_ref().unwrap();
    let desired_spec = desired.spec.as_ref().unwrap();

    existing_spec.replicas != desired_spec.replicas
        || existing_spec.service_name != desired_spec.service_name
        || existing_spec.template.spec.as_ref().unwrap().containers[0].image
            != desired_spec.template.spec.as_ref().unwrap().containers[0].image
        || existing_spec.template.spec.as_ref().unwrap().containers[0].command
            != desired_spec.template.spec.as_ref().unwrap().containers[0].command
        || existing_spec.template.spec.as_ref().unwrap().containers[0].env
            != desired_spec.template.spec.as_ref().unwrap().containers[0].env
        || existing_spec.template.spec.as_ref().unwrap().containers[0].volume_mounts
            != desired_spec.template.spec.as_ref().unwrap().containers[0].volume_mounts
        || existing_spec.volume_claim_templates != desired_spec.volume_claim_templates
}
