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

// SetPhases updates the cluster status using provided functions
func SetPhases(ctx context.Context, c client.Client, cluster *neonv1alpha1.Cluster, statusfns ...func(cluster *neonv1alpha1.Cluster)) error {
	log := logf.FromContext(ctx)

	log.Info("Setting cluster phases")

	originalCluster := cluster.DeepCopy()

	var currentCluster neonv1alpha1.Cluster
	if err := c.Get(ctx, types.NamespacedName{Name: cluster.Name, Namespace: cluster.Namespace}, &currentCluster); err != nil {
		log.Info("Error getting cluster", "error", err)
		return err
	}

	updatedCluster := currentCluster.DeepCopy()

	for _, statusfn := range statusfns {
		statusfn(updatedCluster)
	}

	if equality.Semantic.DeepEqual(originalCluster.Status, updatedCluster.Status) {
		log.Info("Cluster status unchanged")
		return nil
	}

	if err := c.Status().Patch(ctx, updatedCluster, client.MergeFromWithOptions(&currentCluster, client.MergeFromWithOptimisticLock{})); err != nil {
		log.Error(err, "error while updating cluster status")
		return err
	}

	cluster.Status = updatedCluster.Status

	return nil
}

// SetClusterCreatingStatus sets the cluster to creating phase with Ready condition false
func SetClusterCreatingStatus(c *neonv1alpha1.Cluster) {
	c.Status.Phase = neonv1alpha1.ClusterPhaseCreating
	c.Status.Conditions = updateCondition(c.Status.Conditions, metav1.Condition{
		Type:               "Ready",
		Status:             metav1.ConditionFalse,
		Reason:             "ClusterIsNotReady",
		Message:            "Cluster Is Not Ready",
		LastTransitionTime: metav1.Now(),
	})
}

// SetClusterCannotCreateResourcesStatus sets the cluster to cannot create resources phase with Ready condition false
func SetClusterCannotCreateResourcesStatus(c *neonv1alpha1.Cluster) {
	c.Status.Phase = neonv1alpha1.ClusterPhaseCannotCreateClusterResources
	c.Status.Conditions = updateCondition(c.Status.Conditions, metav1.Condition{
		Type:               "Ready",
		Status:             metav1.ConditionFalse,
		Reason:             "ClusterIsNotReady",
		Message:            "Cluster Is Not Ready",
		LastTransitionTime: metav1.Now(),
	})
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
