package utils

import (
	"testing"

	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"sigs.k8s.io/controller-runtime/pkg/client"

	neonv1alpha1 "oltp.molnett.org/neon-operator/api/v1alpha1"
)

const (
	conditionTypeReady = "Ready"
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
		{
			name:   "SafekeeperStatus implements StatusWithConditions",
			status: &neonv1alpha1.SafekeeperStatus{},
		},
		{
			name:   "PageserverStatus implements StatusWithConditions",
			status: &neonv1alpha1.PageserverStatus{},
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
					Type:   conditionTypeReady,
					Status: metav1.ConditionTrue,
					Reason: "TestReason",
				},
			}
			tt.status.SetConditions(conditions)
			gotConditions := tt.status.GetConditions()
			if len(gotConditions) != 1 {
				t.Errorf("expected 1 condition, got %d", len(gotConditions))
			}
			if gotConditions[0].Type != conditionTypeReady {
				t.Errorf("expected condition type '%s', got '%s'", conditionTypeReady, gotConditions[0].Type)
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
		{
			name: "Safekeeper returns SafekeeperStatus",
			obj: &neonv1alpha1.Safekeeper{
				Status: neonv1alpha1.SafekeeperStatus{
					Phase: "safekeeper-phase",
				},
			},
			expected: "safekeeper-phase",
		},
		{
			name: "Pageserver returns PageserverStatus",
			obj: &neonv1alpha1.Pageserver{
				Status: neonv1alpha1.PageserverStatus{
					Phase: "pageserver-phase",
				},
			},
			expected: "pageserver-phase",
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
				Type:   conditionTypeReady,
				Status: metav1.ConditionTrue,
			},
			expected:     1,
			expectUpdate: false,
		},
		{
			name: "Update existing condition",
			existing: []metav1.Condition{
				{
					Type:   conditionTypeReady,
					Status: metav1.ConditionFalse,
					Reason: "OldReason",
				},
			},
			newCondition: metav1.Condition{
				Type:   conditionTypeReady,
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
					Type:   conditionTypeReady,
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
			Type:   conditionTypeReady,
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
		Type:   conditionTypeReady,
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
		if result[i].Type == conditionTypeReady {
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
	if condition.Type != conditionTypeReady {
		t.Errorf("expected condition type '%s', got '%s'", conditionTypeReady, condition.Type)
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
	if condition.Type != conditionTypeReady {
		t.Errorf("expected condition type '%s', got '%s'", conditionTypeReady, condition.Type)
	}
	if condition.Status != metav1.ConditionFalse {
		t.Errorf("expected condition status 'False', got '%s'", condition.Status)
	}
	if condition.Reason != "ClusterIsNotReady" {
		t.Errorf("expected reason 'ClusterIsNotReady', got '%s'", condition.Reason)
	}
}

// Test Cluster Ready Status
func TestSetClusterReadyStatus(t *testing.T) {
	cluster := &neonv1alpha1.Cluster{}

	SetClusterReadyStatus(cluster)

	if cluster.Status.Phase != neonv1alpha1.ClusterPhaseReady {
		t.Errorf("expected phase '%s', got '%s'", neonv1alpha1.ClusterPhaseReady, cluster.Status.Phase)
	}

	if len(cluster.Status.Conditions) != 1 {
		t.Errorf("expected 1 condition, got %d", len(cluster.Status.Conditions))
	}

	condition := cluster.Status.Conditions[0]
	if condition.Type != conditionTypeReady {
		t.Errorf("expected condition type '%s', got '%s'", conditionTypeReady, condition.Type)
	}
	if condition.Status != metav1.ConditionTrue {
		t.Errorf("expected condition status 'True', got '%s'", condition.Status)
	}
	if condition.Reason != "ClusterIsReady" {
		t.Errorf("expected reason 'ClusterIsReady', got '%s'", condition.Reason)
	}
	if condition.Message != "Cluster is ready" {
		t.Errorf("expected message 'Cluster is ready', got '%s'", condition.Message)
	}
}

// Test Safekeeper Status Functions
func TestSetSafekeeperCreatingStatus(t *testing.T) {
	safekeeper := &neonv1alpha1.Safekeeper{}

	SetSafekeeperCreatingStatus(safekeeper)

	if safekeeper.Status.Phase != neonv1alpha1.SafekeeperPhaseCreating {
		t.Errorf("expected phase '%s', got '%s'", neonv1alpha1.SafekeeperPhaseCreating, safekeeper.Status.Phase)
	}

	if len(safekeeper.Status.Conditions) != 1 {
		t.Errorf("expected 1 condition, got %d", len(safekeeper.Status.Conditions))
	}

	condition := safekeeper.Status.Conditions[0]
	if condition.Type != conditionTypeReady {
		t.Errorf("expected condition type '%s', got '%s'", conditionTypeReady, condition.Type)
	}
	if condition.Status != metav1.ConditionFalse {
		t.Errorf("expected condition status 'False', got '%s'", condition.Status)
	}
}

func TestSetSafekeeperInvalidSpecStatus(t *testing.T) {
	safekeeper := &neonv1alpha1.Safekeeper{}

	SetSafekeeperInvalidSpecStatus(safekeeper)

	if safekeeper.Status.Phase != neonv1alpha1.SafekeeperPhaseInvalidSpec {
		t.Errorf("expected phase '%s', got '%s'", neonv1alpha1.SafekeeperPhaseInvalidSpec, safekeeper.Status.Phase)
	}

	if len(safekeeper.Status.Conditions) != 1 {
		t.Errorf("expected 1 condition, got %d", len(safekeeper.Status.Conditions))
	}

	condition := safekeeper.Status.Conditions[0]
	if condition.Type != conditionTypeReady {
		t.Errorf("expected condition type '%s', got '%s'", conditionTypeReady, condition.Type)
	}
	if condition.Status != metav1.ConditionFalse {
		t.Errorf("expected condition status 'False', got '%s'", condition.Status)
	}
	if condition.Reason != "SafekeeperIsNotReady" {
		t.Errorf("expected reason 'SafekeeperIsNotReady', got '%s'", condition.Reason)
	}
}

func TestSetSafekeeperCannotCreateResourcesStatus(t *testing.T) {
	safekeeper := &neonv1alpha1.Safekeeper{}

	SetSafekeeperCannotCreateResourcesStatus(safekeeper)

	if safekeeper.Status.Phase != neonv1alpha1.SafekeeperPhaseCannotCreateResources {
		t.Errorf("expected phase '%s', got '%s'", neonv1alpha1.SafekeeperPhaseCannotCreateResources, safekeeper.Status.Phase)
	}

	if len(safekeeper.Status.Conditions) != 1 {
		t.Errorf("expected 1 condition, got %d", len(safekeeper.Status.Conditions))
	}

	condition := safekeeper.Status.Conditions[0]
	if condition.Type != conditionTypeReady {
		t.Errorf("expected condition type '%s', got '%s'", conditionTypeReady, condition.Type)
	}
	if condition.Status != metav1.ConditionFalse {
		t.Errorf("expected condition status 'False', got '%s'", condition.Status)
	}
	if condition.Reason != "SafekeeperIsNotReady" {
		t.Errorf("expected reason 'SafekeeperIsNotReady', got '%s'", condition.Reason)
	}
}

func TestSetSafekeeperReadyStatus(t *testing.T) {
	safekeeper := &neonv1alpha1.Safekeeper{}

	SetSafekeeperReadyStatus(safekeeper)

	if safekeeper.Status.Phase != neonv1alpha1.SafekeeperPhaseReady {
		t.Errorf("expected phase '%s', got '%s'", neonv1alpha1.SafekeeperPhaseReady, safekeeper.Status.Phase)
	}

	if len(safekeeper.Status.Conditions) != 1 {
		t.Errorf("expected 1 condition, got %d", len(safekeeper.Status.Conditions))
	}

	condition := safekeeper.Status.Conditions[0]
	if condition.Type != conditionTypeReady {
		t.Errorf("expected condition type '%s', got '%s'", conditionTypeReady, condition.Type)
	}
	if condition.Status != metav1.ConditionTrue {
		t.Errorf("expected condition status 'True', got '%s'", condition.Status)
	}
	if condition.Reason != "SafekeeperIsReady" {
		t.Errorf("expected reason 'SafekeeperIsReady', got '%s'", condition.Reason)
	}
	if condition.Message != "Safekeeper is ready" {
		t.Errorf("expected message 'Safekeeper is ready', got '%s'", condition.Message)
	}
}

// Test Pageserver Status Functions
func TestSetPageserverCreatingStatus(t *testing.T) {
	pageserver := &neonv1alpha1.Pageserver{}

	SetPageserverCreatingStatus(pageserver)

	if pageserver.Status.Phase != neonv1alpha1.PageserverPhaseCreating {
		t.Errorf("expected phase '%s', got '%s'", neonv1alpha1.PageserverPhaseCreating, pageserver.Status.Phase)
	}

	if len(pageserver.Status.Conditions) != 1 {
		t.Errorf("expected 1 condition, got %d", len(pageserver.Status.Conditions))
	}

	condition := pageserver.Status.Conditions[0]
	if condition.Type != conditionTypeReady {
		t.Errorf("expected condition type '%s', got '%s'", conditionTypeReady, condition.Type)
	}
	if condition.Status != metav1.ConditionFalse {
		t.Errorf("expected condition status 'False', got '%s'", condition.Status)
	}
}

func TestSetPageserverInvalidSpecStatus(t *testing.T) {
	pageserver := &neonv1alpha1.Pageserver{}

	SetPageserverInvalidSpecStatus(pageserver)

	if pageserver.Status.Phase != neonv1alpha1.PageserverPhaseInvalidSpec {
		t.Errorf("expected phase '%s', got '%s'", neonv1alpha1.PageserverPhaseInvalidSpec, pageserver.Status.Phase)
	}

	if len(pageserver.Status.Conditions) != 1 {
		t.Errorf("expected 1 condition, got %d", len(pageserver.Status.Conditions))
	}

	condition := pageserver.Status.Conditions[0]
	if condition.Type != conditionTypeReady {
		t.Errorf("expected condition type '%s', got '%s'", conditionTypeReady, condition.Type)
	}
	if condition.Status != metav1.ConditionFalse {
		t.Errorf("expected condition status 'False', got '%s'", condition.Status)
	}
	if condition.Reason != "PageserverIsNotReady" {
		t.Errorf("expected reason 'PageserverIsNotReady', got '%s'", condition.Reason)
	}
}

func TestSetPageserverCannotCreateResourcesStatus(t *testing.T) {
	pageserver := &neonv1alpha1.Pageserver{}

	SetPageserverCannotCreateResourcesStatus(pageserver)

	if pageserver.Status.Phase != neonv1alpha1.PageserverPhaseCannotCreateResources {
		t.Errorf("expected phase '%s', got '%s'", neonv1alpha1.PageserverPhaseCannotCreateResources, pageserver.Status.Phase)
	}

	if len(pageserver.Status.Conditions) != 1 {
		t.Errorf("expected 1 condition, got %d", len(pageserver.Status.Conditions))
	}

	condition := pageserver.Status.Conditions[0]
	if condition.Type != conditionTypeReady {
		t.Errorf("expected condition type '%s', got '%s'", conditionTypeReady, condition.Type)
	}
	if condition.Status != metav1.ConditionFalse {
		t.Errorf("expected condition status 'False', got '%s'", condition.Status)
	}
	if condition.Reason != "PageserverIsNotReady" {
		t.Errorf("expected reason 'PageserverIsNotReady', got '%s'", condition.Reason)
	}
}

func TestSetPageserverReadyStatus(t *testing.T) {
	pageserver := &neonv1alpha1.Pageserver{}

	SetPageserverReadyStatus(pageserver)

	if pageserver.Status.Phase != neonv1alpha1.PageserverPhaseReady {
		t.Errorf("expected phase '%s', got '%s'", neonv1alpha1.PageserverPhaseReady, pageserver.Status.Phase)
	}

	if len(pageserver.Status.Conditions) != 1 {
		t.Errorf("expected 1 condition, got %d", len(pageserver.Status.Conditions))
	}

	condition := pageserver.Status.Conditions[0]
	if condition.Type != conditionTypeReady {
		t.Errorf("expected condition type '%s', got '%s'", conditionTypeReady, condition.Type)
	}
	if condition.Status != metav1.ConditionTrue {
		t.Errorf("expected condition status 'True', got '%s'", condition.Status)
	}
	if condition.Reason != "PageserverIsReady" {
		t.Errorf("expected reason 'PageserverIsReady', got '%s'", condition.Reason)
	}
	if condition.Message != "Pageserver is ready" {
		t.Errorf("expected message 'Pageserver is ready', got '%s'", condition.Message)
	}
}

// Test Branch Status Functions
func TestSetBranchCreatingStatus(t *testing.T) {
	branch := &neonv1alpha1.Branch{}

	SetBranchCreatingStatus(branch)

	if branch.Status.Phase != neonv1alpha1.BranchPhaseCreating {
		t.Errorf("expected phase '%s', got '%s'", neonv1alpha1.BranchPhaseCreating, branch.Status.Phase)
	}

	if len(branch.Status.Conditions) != 1 {
		t.Errorf("expected 1 condition, got %d", len(branch.Status.Conditions))
	}

	condition := branch.Status.Conditions[0]
	if condition.Type != conditionTypeReady {
		t.Errorf("expected condition type '%s', got '%s'", conditionTypeReady, condition.Type)
	}
	if condition.Status != metav1.ConditionFalse {
		t.Errorf("expected condition status 'False', got '%s'", condition.Status)
	}
}

func TestSetBranchReadyStatus(t *testing.T) {
	branch := &neonv1alpha1.Branch{}

	SetBranchReadyStatus(branch)

	if branch.Status.Phase != neonv1alpha1.BranchPhaseReady {
		t.Errorf("expected phase '%s', got '%s'", neonv1alpha1.BranchPhaseReady, branch.Status.Phase)
	}

	if len(branch.Status.Conditions) != 1 {
		t.Errorf("expected 1 condition, got %d", len(branch.Status.Conditions))
	}

	condition := branch.Status.Conditions[0]
	if condition.Type != conditionTypeReady {
		t.Errorf("expected condition type '%s', got '%s'", conditionTypeReady, condition.Type)
	}
	if condition.Status != metav1.ConditionTrue {
		t.Errorf("expected condition status 'True', got '%s'", condition.Status)
	}
	if condition.Reason != "BranchIsReady" {
		t.Errorf("expected reason 'BranchIsReady', got '%s'", condition.Reason)
	}
	if condition.Message != "Branch is ready" {
		t.Errorf("expected message 'Branch is ready', got '%s'", condition.Message)
	}
}

func TestSetBranchCannotCreateResourcesStatus(t *testing.T) {
	branch := &neonv1alpha1.Branch{}

	SetBranchCannotCreateResourcesStatus(branch)

	if branch.Status.Phase != neonv1alpha1.BranchPhaseCannotCreateResources {
		t.Errorf("expected phase '%s', got '%s'", neonv1alpha1.BranchPhaseCannotCreateResources, branch.Status.Phase)
	}

	if len(branch.Status.Conditions) != 1 {
		t.Errorf("expected 1 condition, got %d", len(branch.Status.Conditions))
	}

	condition := branch.Status.Conditions[0]
	if condition.Type != conditionTypeReady {
		t.Errorf("expected condition type '%s', got '%s'", conditionTypeReady, condition.Type)
	}
	if condition.Status != metav1.ConditionFalse {
		t.Errorf("expected condition status 'False', got '%s'", condition.Status)
	}
	if condition.Reason != "BranchIsNotReady" {
		t.Errorf("expected reason 'BranchIsNotReady', got '%s'", condition.Reason)
	}
}

// Test Project Status Functions
func TestSetProjectPendingStatus(t *testing.T) {
	project := &neonv1alpha1.Project{}

	SetProjectPendingStatus(project)

	if project.Status.Phase != neonv1alpha1.ProjectPhasePending {
		t.Errorf("expected phase '%s', got '%s'", neonv1alpha1.ProjectPhasePending, project.Status.Phase)
	}

	if len(project.Status.Conditions) != 1 {
		t.Errorf("expected 1 condition, got %d", len(project.Status.Conditions))
	}

	condition := project.Status.Conditions[0]
	if condition.Type != conditionTypeReady {
		t.Errorf("expected condition type '%s', got '%s'", conditionTypeReady, condition.Type)
	}
	if condition.Status != metav1.ConditionFalse {
		t.Errorf("expected condition status 'False', got '%s'", condition.Status)
	}
}

func TestSetProjectCreatingStatus(t *testing.T) {
	project := &neonv1alpha1.Project{}

	SetProjectCreatingStatus(project)

	if project.Status.Phase != neonv1alpha1.ProjectPhaseCreating {
		t.Errorf("expected phase '%s', got '%s'", neonv1alpha1.ProjectPhaseCreating, project.Status.Phase)
	}

	if len(project.Status.Conditions) != 1 {
		t.Errorf("expected 1 condition, got %d", len(project.Status.Conditions))
	}

	condition := project.Status.Conditions[0]
	if condition.Type != conditionTypeReady {
		t.Errorf("expected condition type '%s', got '%s'", conditionTypeReady, condition.Type)
	}
	if condition.Status != metav1.ConditionFalse {
		t.Errorf("expected condition status 'False', got '%s'", condition.Status)
	}
}

func TestSetProjectReadyStatus(t *testing.T) {
	project := &neonv1alpha1.Project{}

	SetProjectReadyStatus(project)

	if project.Status.Phase != neonv1alpha1.ProjectPhaseReady {
		t.Errorf("expected phase '%s', got '%s'", neonv1alpha1.ProjectPhaseReady, project.Status.Phase)
	}

	if len(project.Status.Conditions) != 1 {
		t.Errorf("expected 1 condition, got %d", len(project.Status.Conditions))
	}

	condition := project.Status.Conditions[0]
	if condition.Type != conditionTypeReady {
		t.Errorf("expected condition type '%s', got '%s'", conditionTypeReady, condition.Type)
	}
	if condition.Status != metav1.ConditionTrue {
		t.Errorf("expected condition status 'True', got '%s'", condition.Status)
	}
	if condition.Reason != "ProjectIsReady" {
		t.Errorf("expected reason 'ProjectIsReady', got '%s'", condition.Reason)
	}
	if condition.Message != "Project is ready" {
		t.Errorf("expected message 'Project is ready', got '%s'", condition.Message)
	}
}
