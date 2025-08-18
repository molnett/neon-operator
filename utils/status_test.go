package utils

import (
	"testing"

	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"sigs.k8s.io/controller-runtime/pkg/client"

	neonv1alpha1 "oltp.molnett.org/neon-operator/api/v1alpha1"
)

func TestStatusWithConditions_Interface(t *testing.T) {
	tests := []struct {
		name   string
		status StatusWithConditions
	}{
		{
			name:   "ClusterStatus implements StatusWithConditions",
			status: &neonv1alpha1.ClusterStatus{},
		},
		{
			name:   "ProjectStatus implements StatusWithConditions",
			status: &neonv1alpha1.ProjectStatus{},
		},
		{
			name:   "BranchStatus implements StatusWithConditions",
			status: &neonv1alpha1.BranchStatus{},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			// Test GetPhase/SetPhase
			tt.status.SetPhase("test-phase")
			if got := tt.status.GetPhase(); got != "test-phase" {
				t.Errorf("expected phase 'test-phase', got '%s'", got)
			}

			// Test GetConditions/SetConditions
			conditions := []metav1.Condition{
				{
					Type:   "Ready",
					Status: metav1.ConditionTrue,
					Reason: "TestReason",
				},
			}
			tt.status.SetConditions(conditions)
			gotConditions := tt.status.GetConditions()
			if len(gotConditions) != 1 {
				t.Errorf("expected 1 condition, got %d", len(gotConditions))
			}
			if gotConditions[0].Type != "Ready" {
				t.Errorf("expected condition type 'Ready', got '%s'", gotConditions[0].Type)
			}
		})
	}
}

func TestGetObjectStatus(t *testing.T) {
	tests := []struct {
		name     string
		obj      client.Object
		expected string
	}{
		{
			name: "Cluster returns ClusterStatus",
			obj: &neonv1alpha1.Cluster{
				Status: neonv1alpha1.ClusterStatus{
					Phase: "test-phase",
				},
			},
			expected: "test-phase",
		},
		{
			name: "Project returns ProjectStatus",
			obj: &neonv1alpha1.Project{
				Status: neonv1alpha1.ProjectStatus{
					Phase: "project-phase",
				},
			},
			expected: "project-phase",
		},
		{
			name: "Branch returns BranchStatus",
			obj: &neonv1alpha1.Branch{
				Status: neonv1alpha1.BranchStatus{
					Phase: "branch-phase",
				},
			},
			expected: "branch-phase",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := getObjectStatus(tt.obj)
			if got == nil {
				t.Fatal("expected non-nil status")
			}

			if got.GetPhase() != tt.expected {
				t.Errorf("expected phase '%s', got '%s'", tt.expected, got.GetPhase())
			}
		})
	}
}

func TestGetObjectStatus_UnsupportedType(t *testing.T) {
	// Test with an unsupported type (Pod is a valid client.Object but not one of our CRDs)
	unsupported := &corev1.Pod{}
	result := getObjectStatus(unsupported)
	if result != nil {
		t.Errorf("expected nil for unsupported type, got %v", result)
	}
}

func TestUpdateCondition(t *testing.T) {
	tests := []struct {
		name         string
		existing     []metav1.Condition
		newCondition metav1.Condition
		expected     int
		expectUpdate bool
	}{
		{
			name:     "Add new condition to empty slice",
			existing: []metav1.Condition{},
			newCondition: metav1.Condition{
				Type:   "Ready",
				Status: metav1.ConditionTrue,
			},
			expected:     1,
			expectUpdate: false,
		},
		{
			name: "Update existing condition",
			existing: []metav1.Condition{
				{
					Type:   "Ready",
					Status: metav1.ConditionFalse,
					Reason: "OldReason",
				},
			},
			newCondition: metav1.Condition{
				Type:   "Ready",
				Status: metav1.ConditionTrue,
				Reason: "NewReason",
			},
			expected:     1,
			expectUpdate: true,
		},
		{
			name: "Add new condition type",
			existing: []metav1.Condition{
				{
					Type:   "Ready",
					Status: metav1.ConditionTrue,
				},
			},
			newCondition: metav1.Condition{
				Type:   "Available",
				Status: metav1.ConditionTrue,
			},
			expected:     2,
			expectUpdate: false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := updateCondition(tt.existing, tt.newCondition)

			if len(result) != tt.expected {
				t.Errorf("expected %d conditions, got %d", tt.expected, len(result))
			}

			// Find the updated condition
			var found *metav1.Condition
			for i := range result {
				if result[i].Type == tt.newCondition.Type {
					found = &result[i]
					break
				}
			}

			if found == nil {
				t.Errorf("condition type '%s' not found in result", tt.newCondition.Type)
				return
			}

			if found.Status != tt.newCondition.Status {
				t.Errorf("expected status '%s', got '%s'", tt.newCondition.Status, found.Status)
			}

			if tt.newCondition.Reason != "" && found.Reason != tt.newCondition.Reason {
				t.Errorf("expected reason '%s', got '%s'", tt.newCondition.Reason, found.Reason)
			}
		})
	}
}

func TestUpdateCondition_PreservesOtherConditions(t *testing.T) {
	existing := []metav1.Condition{
		{
			Type:   "Ready",
			Status: metav1.ConditionTrue,
			Reason: "AllGood",
		},
		{
			Type:   "Available",
			Status: metav1.ConditionFalse,
			Reason: "NotAvailable",
		},
	}

	newCondition := metav1.Condition{
		Type:   "Ready",
		Status: metav1.ConditionFalse,
		Reason: "NotReady",
	}

	result := updateCondition(existing, newCondition)

	if len(result) != 2 {
		t.Errorf("expected 2 conditions, got %d", len(result))
	}

	// Check that Available condition is preserved
	var availableCondition *metav1.Condition
	var readyCondition *metav1.Condition

	for i := range result {
		if result[i].Type == "Available" {
			availableCondition = &result[i]
		}
		if result[i].Type == "Ready" {
			readyCondition = &result[i]
		}
	}

	if availableCondition == nil {
		t.Error("Available condition should be preserved")
	} else {
		if availableCondition.Status != metav1.ConditionFalse {
			t.Errorf("Available condition status should be preserved as False, got %s", availableCondition.Status)
		}
		if availableCondition.Reason != "NotAvailable" {
			t.Errorf("Available condition reason should be preserved as 'NotAvailable', got '%s'", availableCondition.Reason)
		}
	}

	if readyCondition == nil {
		t.Error("Ready condition should exist")
	} else {
		if readyCondition.Status != metav1.ConditionFalse {
			t.Errorf("Ready condition should be updated to False, got %s", readyCondition.Status)
		}
		if readyCondition.Reason != "NotReady" {
			t.Errorf("Ready condition should be updated to 'NotReady', got '%s'", readyCondition.Reason)
		}
	}
}

func TestSetClusterCreatingStatus(t *testing.T) {
	cluster := &neonv1alpha1.Cluster{}

	SetClusterCreatingStatus(cluster)

	if cluster.Status.Phase != neonv1alpha1.ClusterPhaseCreating {
		t.Errorf("expected phase '%s', got '%s'", neonv1alpha1.ClusterPhaseCreating, cluster.Status.Phase)
	}

	if len(cluster.Status.Conditions) != 1 {
		t.Errorf("expected 1 condition, got %d", len(cluster.Status.Conditions))
	}

	condition := cluster.Status.Conditions[0]
	if condition.Type != "Ready" {
		t.Errorf("expected condition type 'Ready', got '%s'", condition.Type)
	}
	if condition.Status != metav1.ConditionFalse {
		t.Errorf("expected condition status 'False', got '%s'", condition.Status)
	}
}

func TestSetClusterCannotCreateResourcesStatus(t *testing.T) {
	cluster := &neonv1alpha1.Cluster{}

	SetClusterCannotCreateResourcesStatus(cluster)

	if cluster.Status.Phase != neonv1alpha1.ClusterPhaseCannotCreateClusterResources {
		t.Errorf("expected phase '%s', got '%s'", neonv1alpha1.ClusterPhaseCannotCreateClusterResources, cluster.Status.Phase)
	}

	if len(cluster.Status.Conditions) != 1 {
		t.Errorf("expected 1 condition, got %d", len(cluster.Status.Conditions))
	}

	condition := cluster.Status.Conditions[0]
	if condition.Type != "Ready" {
		t.Errorf("expected condition type 'Ready', got '%s'", condition.Type)
	}
	if condition.Status != metav1.ConditionFalse {
		t.Errorf("expected condition status 'False', got '%s'", condition.Status)
	}
	if condition.Reason != "ClusterIsNotReady" {
		t.Errorf("expected reason 'ClusterIsNotReady', got '%s'", condition.Reason)
	}
}
