use k8s_openapi::serde_json::{json, Value};

/// Updates a compute spec JSON string with new pageserver connection string and optional stripe size
pub fn update_compute_spec_json(
    spec_json: &str,
    pageserver_connstring: &str,
    stripe_size: Option<u32>,
) -> Result<String, String> {
    let mut spec: Value = k8s_openapi::serde_json::from_str(spec_json)
        .map_err(|e| format!("Failed to parse spec.json: {}", e))?;

    // Get mutable reference to settings array, fail if structure is invalid
    let cluster = spec["spec"]["cluster"]
        .as_object_mut()
        .ok_or("spec.cluster is not an object")?;

    let settings = cluster["settings"]
        .as_array_mut()
        .ok_or("spec.cluster.settings is not an array")?;

    // Update neon.pageserver_connstring - must find and update it
    let mut found_pageserver_connstring = false;
    for setting in settings.iter_mut() {
        let name = setting["name"].as_str().ok_or("setting.name is not a string")?;

        if name == "neon.pageserver_connstring" {
            setting["value"] = json!(pageserver_connstring);
            found_pageserver_connstring = true;
        } else if name == "neon.shard_stripe_size" && stripe_size.is_some() {
            setting["value"] = json!(stripe_size.unwrap().to_string());
        }
    }

    if !found_pageserver_connstring {
        return Err("neon.pageserver_connstring setting not found in compute spec".to_string());
    }

    // Add neon.shard_stripe_size if it doesn't exist and stripe_size is provided
    if let Some(stripe_size) = stripe_size {
        let has_stripe_size = settings
            .iter()
            .any(|s| s["name"].as_str() == Some("neon.shard_stripe_size"));

        if !has_stripe_size {
            settings.push(json!({
                "name": "neon.shard_stripe_size",
                "value": stripe_size.to_string(),
                "vartype": "integer"
            }));
        }
    }

    k8s_openapi::serde_json::to_string_pretty(&spec)
        .map_err(|e| format!("Failed to serialize updated spec: {}", e))
}