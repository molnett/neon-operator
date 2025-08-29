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

// PageserverSpec defines the desired state of Pageserver
type PageserverSpec struct {
	// ID which the pageserver uses when registering with storage-controller
	// This ID must be unique within the cluster.
	ID uint64 `json:"id"`

	// Used to deterministically setup which storage controller and broker to communicate with
	Cluster string `json:"cluster"`

	// Reference to a Secret containing credentials for accessing a storage bucket.
	BucketCredentialsSecret *corev1.SecretReference `json:"bucketCredentialsSecret"`

	// PVC configuration
	StorageConfig StorageConfig `json:"storageConfig"`
}

// PageserverStatus defines the observed state of Pageserver.
type PageserverStatus struct {
	Conditions []metav1.Condition `json:"conditions"`

	Phase       string `json:"phase"`
	PhaseReason string `json:"phaseReason,omitempty"`
}

// GetConditions returns the conditions for the pageserver status
func (b *PageserverStatus) GetConditions() []metav1.Condition {
	return b.Conditions
}

// SetConditions sets the conditions for the pageserver status
func (b *PageserverStatus) SetConditions(conditions []metav1.Condition) {
	b.Conditions = conditions
}

// GetPhase returns the phase for the pageserver status
func (b *PageserverStatus) GetPhase() string {
	return b.Phase
}

// SetPhase sets the phase for the pageserver status
func (b *PageserverStatus) SetPhase(phase string) {
	b.Phase = phase
}

const (
	PageserverPhaseCreating              = "Creating pageserver"
	PageserverPhaseInvalidSpec           = "Invalid spec"
	PageserverPhaseCannotCreateResources = "Unable to create all necessary pageserver resources"
)

// +kubebuilder:object:root=true
// +kubebuilder:subresource:status

// Pageserver is the Schema for the pageservers API
type Pageserver struct {
	metav1.TypeMeta `json:",inline"`

	// metadata is a standard object metadata
	// +optional
	metav1.ObjectMeta `json:"metadata,omitzero"`

	// spec defines the desired state of Pageserver
	// +required
	Spec PageserverSpec `json:"spec"`

	// status defines the observed state of Pageserver
	// +optional
	Status PageserverStatus `json:"status,omitzero"`
}

// +kubebuilder:object:root=true

// PageserverList contains a list of Pageserver
type PageserverList struct {
	metav1.TypeMeta `json:",inline"`
	metav1.ListMeta `json:"metadata"`
	Items           []Pageserver `json:"items"`
}

func init() {
	SchemeBuilder.Register(&Pageserver{}, &PageserverList{})
}
