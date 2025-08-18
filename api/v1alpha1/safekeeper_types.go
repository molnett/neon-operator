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
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
)

type StorageConfig struct {
	// Name of the storage class to use for PVCs.
	// +optional
	StorageClass *string `json:"storageClass,omitempty"`

	// Size of the PVCs.
	// kubebuilder:default:=10Gi
	Size string `json:"size"`
}

// SafekeeperSpec defines the desired state of Safekeeper
type SafekeeperSpec struct {
	// ID which the safekeepers uses when registering with storage-controller
	ID uint32 `json:"id"`

	// Used to deterministically setup which storage controller and broker to communicate with
	Cluster string `json:"cluster"`

	// PVC configuration
	StorageConfig StorageConfig `json:"storageConfig"`
}

// SafekeeperStatus defines the observed state of Safekeeper.
type SafekeeperStatus struct {
	Conditions []metav1.Condition `json:"conditions"`

	Phase       string `json:"phase"`
	PhaseReason string `json:"phaseReason,omitempty"`
}

const (
	SafekeeperPhaseCreating              = "Creating safekeeper"
	SafekeeperPhaseInvalidSpec           = "Invalid spec"
	SafekeeperPhaseCannotCreateResources = "Unable to create all necessary safekeeper resources"
)

// +kubebuilder:object:root=true
// +kubebuilder:subresource:status

// Safekeeper is the Schema for the safekeepers API
type Safekeeper struct {
	metav1.TypeMeta `json:",inline"`

	// metadata is a standard object metadata
	// +optional
	metav1.ObjectMeta `json:"metadata,omitzero"`

	// spec defines the desired state of Safekeeper
	// +required
	Spec SafekeeperSpec `json:"spec"`

	// status defines the observed state of Safekeeper
	// +optional
	Status SafekeeperStatus `json:"status,omitzero"`
}

// GetConditions returns the conditions for the safekeeper status
func (b *SafekeeperStatus) GetConditions() []metav1.Condition {
	return b.Conditions
}

// SetConditions sets the conditions for the safekeeper status
func (b *SafekeeperStatus) SetConditions(conditions []metav1.Condition) {
	b.Conditions = conditions
}

// GetPhase returns the phase for the safekeeper status
func (b *SafekeeperStatus) GetPhase() string {
	return b.Phase
}

// SetPhase sets the phase for the safekeeper status
func (b *SafekeeperStatus) SetPhase(phase string) {
	b.Phase = phase
}

// +kubebuilder:object:root=true

// SafekeeperList contains a list of Safekeeper
type SafekeeperList struct {
	metav1.TypeMeta `json:",inline"`
	metav1.ListMeta `json:"metadata"`
	Items           []Safekeeper `json:"items"`
}

func init() {
	SchemeBuilder.Register(&Safekeeper{}, &SafekeeperList{})
}
