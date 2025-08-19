/*
Copyright 2025.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
*/

package utils

import (
	"context"

	"k8s.io/apimachinery/pkg/api/equality"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/apimachinery/pkg/types"
	"sigs.k8s.io/controller-runtime/pkg/client"
	logf "sigs.k8s.io/controller-runtime/pkg/log"

	neonv1alpha1 "oltp.molnett.org/neon-operator/api/v1alpha1"
)

// StatusWithConditions defines the interface for status objects that have Conditions and Phase
type StatusWithConditions interface {
	GetConditions() []metav1.Condition
	SetConditions([]metav1.Condition)
	GetPhase() string
	SetPhase(string)
}

// SetPhases updates the resource status using provided functions
func SetPhases[T client.Object](ctx context.Context, c client.Client, obj T, statusfns ...func(T)) error {
	log := logf.FromContext(ctx)

	log.Info("Setting resource phases")

	original := obj.DeepCopyObject().(T)

	current := obj.DeepCopyObject().(T)
	if err := c.Get(ctx, types.NamespacedName{Name: obj.GetName(), Namespace: obj.GetNamespace()}, current); err != nil {
		log.Info("Error getting resource", "error", err)
		return err
	}

	updated := current.DeepCopyObject().(T)

	for _, statusfn := range statusfns {
		statusfn(updated)
	}

	originalStatus := getOriginalStatusForComparison(original)
	updatedStatus := getOriginalStatusForComparison(updated)

	if equality.Semantic.DeepEqual(originalStatus, updatedStatus) {
		log.Info("Resource status unchanged")
		return nil
	}

	if err := c.Status().Patch(ctx, updated, client.MergeFromWithOptions(current, client.MergeFromWithOptimisticLock{})); err != nil {
		log.Error(err, "error while updating resource status")
		return err
	}

	copyUpdatedStatus(obj, updated)

	return nil
}

// getOriginalStatusForComparison extracts status for comparison
func getOriginalStatusForComparison(obj client.Object) any {
	switch v := obj.(type) {
	case *neonv1alpha1.Cluster:
		return v.Status
	case *neonv1alpha1.Project:
		return v.Status
	case *neonv1alpha1.Branch:
		return v.Status
	case *neonv1alpha1.Safekeeper:
		return v.Status
	case *neonv1alpha1.Pageserver:
		return v.Status
	default:
		return nil
	}
}

// copyUpdatedStatus copies the status from updated object to the original
func copyUpdatedStatus(original client.Object, updated client.Object) {
	switch orig := original.(type) {
	case *neonv1alpha1.Cluster:
		if upd, ok := updated.(*neonv1alpha1.Cluster); ok {
			orig.Status = upd.Status
		}
	case *neonv1alpha1.Project:
		if upd, ok := updated.(*neonv1alpha1.Project); ok {
			orig.Status = upd.Status
		}
	case *neonv1alpha1.Branch:
		if upd, ok := updated.(*neonv1alpha1.Branch); ok {
			orig.Status = upd.Status
		}
	case *neonv1alpha1.Safekeeper:
		if upd, ok := updated.(*neonv1alpha1.Safekeeper); ok {
			orig.Status = upd.Status
		}
	case *neonv1alpha1.Pageserver:
		if upd, ok := updated.(*neonv1alpha1.Pageserver); ok {
			orig.Status = upd.Status
		}
	}
}

// getObjectStatus extracts the status from any of our CRD objects
func getObjectStatus(obj client.Object) StatusWithConditions {
	switch v := obj.(type) {
	case *neonv1alpha1.Cluster:
		return &v.Status
	case *neonv1alpha1.Project:
		return &v.Status
	case *neonv1alpha1.Branch:
		return &v.Status
	case *neonv1alpha1.Safekeeper:
		return &v.Status
	case *neonv1alpha1.Pageserver:
		return &v.Status
	default:
		return nil
	}
}

// SetPhase sets a generic creating phase with Ready condition false
func SetPhase[T client.Object](obj T, phase string) {
	status := getObjectStatus(obj)
	status.SetPhase(phase)
	status.SetConditions(updateCondition(status.GetConditions(), metav1.Condition{
		Type:               "Ready",
		Status:             metav1.ConditionFalse,
		Reason:             "ResourceIsNotReady",
		Message:            "Resource Is Not Ready",
		LastTransitionTime: metav1.Now(),
	}))

}

// SetError sets a generic error phase with Ready condition false and error message
func SetError[T client.Object](obj T, phase, reason, message string) {
	status := getObjectStatus(obj)
	status.SetPhase(phase)
	status.SetConditions(updateCondition(status.GetConditions(), metav1.Condition{
		Type:               "Ready",
		Status:             metav1.ConditionFalse,
		Reason:             reason,
		Message:            message,
		LastTransitionTime: metav1.Now(),
	}))
}

// SetClusterCreatingStatus sets the cluster to creating phase with Ready condition false
func SetClusterCreatingStatus(c *neonv1alpha1.Cluster) {
	SetPhase(c, neonv1alpha1.ClusterPhaseCreating)
}

// SetClusterCannotCreateResourcesStatus sets the cluster to cannot create resources phase with Ready condition false
func SetClusterCannotCreateResourcesStatus(c *neonv1alpha1.Cluster) {
	SetError(c, neonv1alpha1.ClusterPhaseCannotCreateClusterResources, "ClusterIsNotReady", "Cluster Is Not Ready")
}

// SetSafekeeperCreatingStatus sets the safekeeper to creating phase with Ready condition false
func SetSafekeeperCreatingStatus(sk *neonv1alpha1.Safekeeper) {
	SetPhase(sk, neonv1alpha1.SafekeeperPhaseCreating)
}

// SetSafekeeperInvalidSpecStatus sets the safekeeper to cannot create resources phase with Ready condition false
func SetSafekeeperInvalidSpecStatus(c *neonv1alpha1.Safekeeper) {
	SetError(c, neonv1alpha1.SafekeeperPhaseInvalidSpec, "SafekeeperIsNotReady", "Safekeeper Is Not Ready")
}

// SetSafekeeperCannotCreateResourcesStatus sets the cluster to cannot create resources phase with Ready condition false
func SetSafekeeperCannotCreateResourcesStatus(c *neonv1alpha1.Safekeeper) {
	SetError(c, neonv1alpha1.SafekeeperPhaseCannotCreateResources, "SafekeeperIsNotReady", "Safekeeper Is Not Ready")
}

// SetPageserverCreatingStatus sets the pageserver to creating phase with Ready condition false
func SetPageserverCreatingStatus(ps *neonv1alpha1.Pageserver) {
	SetPhase(ps, neonv1alpha1.PageserverPhaseCreating)
}

// SetPageserverInvalidSpecStatus sets the pageserver to cannot create resources phase with Ready condition false
func SetPageserverInvalidSpecStatus(ps *neonv1alpha1.Pageserver) {
	SetError(ps, neonv1alpha1.PageserverPhaseInvalidSpec, "PageserverIsNotReady", "Pageserver Is Not Ready")
}

// SetPageserverCannotCreateResourcesStatus sets the pageserver to cannot create resources phase with Ready condition false
func SetPageserverCannotCreateResourcesStatus(ps *neonv1alpha1.Pageserver) {
	SetError(ps, neonv1alpha1.PageserverPhaseCannotCreateResources, "PageserverIsNotReady", "Pageserver Is Not Ready")
}

// SetProjectPendingStatus sets the project to pending phase with Ready condition false
func SetProjectPendingStatus(p *neonv1alpha1.Project) {
	SetPhase(p, neonv1alpha1.ProjectPhasePending)
}

// SetProjectCreatingStatus sets the project to creating phase with Ready condition false
func SetProjectCreatingStatus(p *neonv1alpha1.Project) {
	SetPhase(p, neonv1alpha1.ProjectPhaseCreating)
}

// SetProjectReadyStatus sets the project to ready phase with Ready condition true
func SetProjectReadyStatus(p *neonv1alpha1.Project) {
	status := getObjectStatus(p)
	status.SetPhase(neonv1alpha1.ProjectPhaseReady)
	status.SetConditions(updateCondition(status.GetConditions(), metav1.Condition{
		Type:               "Ready",
		Status:             metav1.ConditionTrue,
		Reason:             "ProjectIsReady",
		Message:            "Project is ready",
		LastTransitionTime: metav1.Now(),
	}))
}

// SetProjectTenantCreationFailedStatus sets the project to tenant creation failed phase with Ready condition false
func SetProjectTenantCreationFailedStatus(p *neonv1alpha1.Project, message string) {
	SetError(p, neonv1alpha1.ProjectPhaseTenantCreationFailed, "TenantCreationFailed", message)
}

// SetProjectPageserverConnectionErrorStatus sets the project to pageserver connection error phase with Ready condition false
func SetProjectPageserverConnectionErrorStatus(p *neonv1alpha1.Project, message string) {
	SetError(p, neonv1alpha1.ProjectPhasePageserverConnectionError, "PageserverConnectionError", message)
}

// SetBranchCreatingStatus sets the branch to creating phase with Ready condition false
func SetBranchCreatingStatus(b *neonv1alpha1.Branch) {
	SetPhase(b, neonv1alpha1.BranchPhaseCreating)
}

// SetBranchReadyStatus sets the branch to ready phase with Ready condition true
func SetBranchReadyStatus(b *neonv1alpha1.Branch) {
	status := getObjectStatus(b)
	status.SetPhase(neonv1alpha1.BranchPhaseReady)
	status.SetConditions(updateCondition(status.GetConditions(), metav1.Condition{
		Type:               "Ready",
		Status:             metav1.ConditionTrue,
		Reason:             "BranchIsReady",
		Message:            "Branch is ready",
		LastTransitionTime: metav1.Now(),
	}))
}

// SetBranchCannotCreateResourcesStatus sets the branch to cannot create resources phase with Ready condition false
func SetBranchCannotCreateResourcesStatus(b *neonv1alpha1.Branch) {
	SetError(b, neonv1alpha1.BranchPhaseCannotCreateResources, "BranchIsNotReady", "Branch Is Not Ready")
}

// SetBranchTimelineCreationFailedStatus sets the branch to timeline creation failed phase with Ready condition false
func SetBranchTimelineCreationFailedStatus(b *neonv1alpha1.Branch, message string) {
	SetError(b, neonv1alpha1.BranchPhaseTimelineCreationFailed, "TimelineCreationFailed", message)
}

// updateCondition updates or adds a condition to the conditions slice
func updateCondition(conditions []metav1.Condition, newCondition metav1.Condition) []metav1.Condition {
	// Find existing condition with the same type
	for i, condition := range conditions {
		if condition.Type == newCondition.Type {
			// Update existing condition
			conditions[i] = newCondition
			return conditions
		}
	}

	// Add new condition if not found
	return append(conditions, newCondition)
}
