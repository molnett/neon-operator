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

// ProjectSpec defines the desired state of Project
type ProjectSpec struct {
	// Name of the cluster where the project will be created.
	ClusterName string `json:"cluster"`

	// Will be generated unless specified.
	// Has to be a 32 character alphanumeric string.
	// +optional
	TenantID string `json:"tenantId"`

	// PostgreSQL version to use for the project.
	// +optional
	PGVersion int `json:"pgVersion"`
}

// ProjectStatus defines the observed state of Project.
type ProjectStatus struct {
	Conditions []metav1.Condition `json:"conditions,omitempty,omitzero"`

	Phase string `json:"phase,omitempty"`
}

const (
	ProjectPhasePending                   = "Pending"
	ProjectPhaseCreating                  = "Creating"
	ProjectPhaseReady                     = "Ready"
	ProjectPhaseTenantCreationFailed      = "TenantCreationFailed"
	ProjectPhasePageserverConnectionError = "PageserverConnectionError"
)

// +kubebuilder:object:root=true
// +kubebuilder:subresource:status

// Project is the Schema for the projects API
type Project struct {
	metav1.TypeMeta `json:",inline"`

	// metadata is a standard object metadata
	// +optional
	metav1.ObjectMeta `json:"metadata,omitempty,omitzero"`

	// spec defines the desired state of Project
	// +required
	Spec ProjectSpec `json:"spec"`

	// status defines the observed state of Project
	// +optional
	Status ProjectStatus `json:"status,omitempty,omitzero"`
}

// +kubebuilder:object:root=true

// ProjectList contains a list of Project
type ProjectList struct {
	metav1.TypeMeta `json:",inline"`
	metav1.ListMeta `json:"metadata,omitempty"`
	Items           []Project `json:"items"`
}

// GetConditions returns the conditions for the project status
func (p *ProjectStatus) GetConditions() []metav1.Condition {
	return p.Conditions
}

// SetConditions sets the conditions for the project status
func (p *ProjectStatus) SetConditions(conditions []metav1.Condition) {
	p.Conditions = conditions
}

// GetPhase returns the phase for the project status
func (p *ProjectStatus) GetPhase() string {
	return p.Phase
}

// SetPhase sets the phase for the project status
func (p *ProjectStatus) SetPhase(phase string) {
	p.Phase = phase
}

func init() {
	SchemeBuilder.Register(&Project{}, &ProjectList{})
}
