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

// BranchSpec defines the desired state of Branch
type BranchSpec struct {
	ID   string `json:"id"`
	Name string `json:"name"`

	// Will be generated unless specified.
	// Has to be a 32 character alphanumeric string.
	// +optional
	TimelineID string `json:"timelineID"`

	// PGVersion specifies the PostgreSQL version to use for the branch.
	// kubebuilder:validation:Enum=14;15;16;17
	// kubebuilder:default:=17
	PGVersion string `json:"pgVersion"`

	// The ID of the Project this Branch belongs to
	ProjectID string `json:"projectID"`
}

// BranchStatus defines the observed state of Branch.
type BranchStatus struct {
	Conditions []metav1.Condition `json:"conditions,omitempty,omitzero"`

	Phase string `json:"phase,omitempty"`
}

// +kubebuilder:object:root=true
// +kubebuilder:subresource:status

// Branch is the Schema for the branches API
type Branch struct {
	metav1.TypeMeta `json:",inline"`

	// metadata is a standard object metadata
	// +optional
	metav1.ObjectMeta `json:"metadata,omitempty,omitzero"`

	// spec defines the desired state of Branch
	// +required
	Spec BranchSpec `json:"spec"`

	// status defines the observed state of Branch
	// +optional
	Status BranchStatus `json:"status,omitempty,omitzero"`
}

// +kubebuilder:object:root=true

// BranchList contains a list of Branch
type BranchList struct {
	metav1.TypeMeta `json:",inline"`
	metav1.ListMeta `json:"metadata,omitempty"`
	Items           []Branch `json:"items"`
}

// GetConditions returns the conditions for the branch status
func (b *BranchStatus) GetConditions() []metav1.Condition {
	return b.Conditions
}

// SetConditions sets the conditions for the branch status
func (b *BranchStatus) SetConditions(conditions []metav1.Condition) {
	b.Conditions = conditions
}

// GetPhase returns the phase for the branch status
func (b *BranchStatus) GetPhase() string {
	return b.Phase
}

// SetPhase sets the phase for the branch status
func (b *BranchStatus) SetPhase(phase string) {
	b.Phase = phase
}

func init() {
	SchemeBuilder.Register(&Branch{}, &BranchList{})
}
