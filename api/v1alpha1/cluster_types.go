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

package v1alpha1

import (
	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
)

// ClusterSpec defines the desired state of Cluster
type ClusterSpec struct {
	// Decides how many safekeepers to run in the cluster.
	// +kubebuilder:default:=3
	// +kubebuilder:validation:Minimum:=3
	NumSafekeepers uint8 `json:"numSafekeepers"`

	// Default PostgreSQL version to use if no version is specified in projects.
	// kubebuilder:validation:Enum=14;15;16;17
	DefaultPGVersion string `json:"defaultPGVersion"`

	// The default Neon image to use for all neon-specific resources.
	// +kubebuilder:default:="neondatabase/neon:8463"
	NeonImage string `json:"neonImage"`

	// Reference to a Secret containing credentials for accessing a storage bucket.
	BucketCredentialsSecret *corev1.SecretReference `json:"bucketCredentialsSecret"`

	// Reference to a Secret containing credentials for accessing a storage bucket.
	// Must have a field named "uri"
	StorageControllerDatabaseSecret *corev1.SecretKeySelector `json:"storageControllerDatabaseSecret"`
}

// ClusterStatus defines the observed state of Cluster.
type ClusterStatus struct {
	Conditions []metav1.Condition `json:"conditions,omitempty"`

	// +optional
	StorageBrokerStatus StorageBrokerStatus `json:"storageBrokerStatus,omitzero"`

	// +optional
	Phase string `json:"phase,omitempty"`

	// +optional
	PhaseReason string `json:"phaseReason,omitempty"`
}

const (
	ClusterPhaseCreating                     = "Creating cluster"
	ClusterPhaseCannotCreateClusterResources = "Unable to create all necessary cluster resources"
)

// Represents the current state of the storage broker.
type StorageBrokerStatus struct {
	// Total number of ready Storage Brokers instances in the cluster.
	ReadyInstances int32 `json:"readyInstances"`
}

// +kubebuilder:object:root=true
// +kubebuilder:subresource:status

// Cluster is the Schema for the clusters API
type Cluster struct {
	metav1.TypeMeta `json:",inline"`

	// metadata is a standard object metadata
	metav1.ObjectMeta `json:"metadata"`

	// spec defines the desired state of Cluster
	// +required
	Spec ClusterSpec `json:"spec"`

	// status defines the observed state of Cluster
	// ReadOnly
	// +optional
	Status ClusterStatus `json:"status,omitzero"`
}

// +kubebuilder:object:root=true

// ClusterList contains a list of Cluster
type ClusterList struct {
	metav1.TypeMeta `json:",inline"`
	metav1.ListMeta `json:"metadata,omitzero"`
	Items           []Cluster `json:"items"`
}

// GetConditions returns the conditions for the cluster status
func (c *ClusterStatus) GetConditions() []metav1.Condition {
	return c.Conditions
}

// SetConditions sets the conditions for the cluster status
func (c *ClusterStatus) SetConditions(conditions []metav1.Condition) {
	c.Conditions = conditions
}

// GetPhase returns the phase for the cluster status
func (c *ClusterStatus) GetPhase() string {
	return c.Phase
}

// SetPhase sets the phase for the cluster status
func (c *ClusterStatus) SetPhase(phase string) {
	c.Phase = phase
}

func init() {
	SchemeBuilder.Register(&Cluster{}, &ClusterList{})
}
