use chrono::Utc;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{Condition, Time};

/// Sets the corresponding condition in conditions to new_condition and returns
/// a tuple containing the new conditions vector and whether it was changed.
///
/// 1. If the condition of the specified type already exists, all fields of the existing condition
///    are updated to new_condition. LastTransitionTime is set to now if the new status differs
///    from the old status
/// 2. If a condition of the specified type does not exist, LastTransitionTime is set to now()
///    if unset, and new_condition is appended
pub fn set_status_condition(
    conditions: &[Condition],
    mut new_condition: Condition,
) -> (Vec<Condition>, bool) {
    let mut new_conditions = Vec::from(conditions);
    let mut changed = false;

    if let Some(index) = new_conditions.iter().position(|c| c.type_ == new_condition.type_) {
        // Update existing condition
        let existing = &mut new_conditions[index];

        if existing.status != new_condition.status {
            existing.status = new_condition.status;
            existing.last_transition_time = Time(Utc::now());
            changed = true;
        }

        if existing.reason != new_condition.reason {
            existing.reason = new_condition.reason;
            changed = true;
        }

        if existing.message != new_condition.message {
            existing.message = new_condition.message;
            changed = true;
        }

        if existing.observed_generation != new_condition.observed_generation {
            existing.observed_generation = new_condition.observed_generation;
            changed = true;
        }
    } else {
        // Add new condition
        new_condition.last_transition_time = Time(Utc::now());
        new_conditions.push(new_condition);
        changed = true;
    }

    (new_conditions, changed)
}

/// Removes the corresponding condition_type from conditions if present.
/// Returns a tuple containing the new conditions vector and whether any condition was removed.
pub fn remove_status_condition(conditions: &[Condition], condition_type: &str) -> (Vec<Condition>, bool) {
    let mut new_conditions = conditions.to_vec();
    let original_len = new_conditions.len();
    new_conditions.retain(|condition| condition.type_ != condition_type);
    let removed = new_conditions.len() != original_len;
    (new_conditions, removed)
}

/// Finds the condition_type in conditions.
pub fn find_status_condition<'a>(conditions: &'a [Condition], condition_type: &str) -> Option<&'a Condition> {
    conditions
        .iter()
        .find(|condition| condition.type_ == condition_type)
}

/// Finds the condition_type in conditions and returns a mutable reference.
pub fn find_status_condition_mut<'a>(
    conditions: &'a mut [Condition],
    condition_type: &str,
) -> Option<&'a mut Condition> {
    conditions
        .iter_mut()
        .find(|condition| condition.type_ == condition_type)
}

/// Returns true when the condition_type is present and set to `True`
pub fn is_status_condition_true(conditions: &[Condition], condition_type: &str) -> bool {
    is_status_condition_present_and_equal(conditions, condition_type, "True")
}

/// Returns true when the condition_type is present and set to `False`
pub fn is_status_condition_false(conditions: &[Condition], condition_type: &str) -> bool {
    is_status_condition_present_and_equal(conditions, condition_type, "False")
}

/// Returns true when condition_type is present and equal to status.
pub fn is_status_condition_present_and_equal(
    conditions: &[Condition],
    condition_type: &str,
    status: &str,
) -> bool {
    conditions
        .iter()
        .any(|condition| condition.type_ == condition_type && condition.status == status)
}

#[cfg(test)]
mod tests {
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;

    use super::*;

    #[test]
    fn test_set_status_condition() {
        let conditions = Vec::new();

        // Test adding new condition
        let condition = Condition {
            type_: "Ready".to_string(),
            status: "True".to_string(),
            reason: "Testing".to_string(),
            message: "Test message".to_string(),
            last_transition_time: Time(Utc::now()),
            observed_generation: Some(1),
        };

        let (conditions, changed) = set_status_condition(&conditions, condition);
        assert!(changed);
        assert_eq!(conditions.len(), 1);

        // Test updating existing condition
        let updated_condition = Condition {
            type_: "Ready".to_string(),
            status: "False".to_string(),
            reason: "UpdatedReason".to_string(),
            message: "Updated message".to_string(),
            last_transition_time: Time(Utc::now()),
            observed_generation: Some(2),
        };

        let (conditions, changed) = set_status_condition(&conditions, updated_condition);
        assert!(changed);
        assert_eq!(conditions.len(), 1);
        assert_eq!(conditions[0].status, "False");
    }

    #[test]
    fn test_remove_status_condition() {
        let conditions = vec![Condition {
            type_: "Ready".to_string(),
            status: "True".to_string(),
            reason: "Testing".to_string(),
            message: "Test message".to_string(),
            last_transition_time: Time(Utc::now()),
            observed_generation: Some(1),
        }];

        let (conditions, removed) = remove_status_condition(&conditions, "Ready");
        assert!(removed);
        assert!(conditions.is_empty());
    }
}
