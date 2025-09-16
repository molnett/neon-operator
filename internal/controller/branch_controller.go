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

package controller

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"time"

	appsv1 "k8s.io/api/apps/v1"
	corev1 "k8s.io/api/core/v1"
	apierrors "k8s.io/apimachinery/pkg/api/errors"
	"k8s.io/apimachinery/pkg/runtime"
	"k8s.io/apimachinery/pkg/types"
	ctrl "sigs.k8s.io/controller-runtime"
	"sigs.k8s.io/controller-runtime/pkg/client"
	logf "sigs.k8s.io/controller-runtime/pkg/log"

	neonv1alpha1 "oltp.molnett.org/neon-operator/api/v1alpha1"
	"oltp.molnett.org/neon-operator/utils"
)

// BranchReconciler reconciles a Branch object
type BranchReconciler struct {
	client.Client
	Scheme *runtime.Scheme
}

// +kubebuilder:rbac:groups=neon.oltp.molnett.org,resources=branches,verbs=get;list;watch;create;update;patch;delete
// +kubebuilder:rbac:groups=neon.oltp.molnett.org,resources=branches/status,verbs=get;update;patch
// +kubebuilder:rbac:groups=neon.oltp.molnett.org,resources=branches/finalizers,verbs=update
// +kubebuilder:rbac:groups=neon.oltp.molnett.org,resources=projects,verbs=get;list;watch
// +kubebuilder:rbac:groups=neon.oltp.molnett.org,resources=clusters,verbs=get;list;watch
// +kubebuilder:rbac:groups="",resources=persistentvolumeclaims,verbs=get;list;watch;create;update;patch;delete
// +kubebuilder:rbac:groups="",resources=pods,verbs=get;list;watch;create;update;patch;delete
// +kubebuilder:rbac:groups="",resources=services,verbs=get;list;watch;create;update;patch;delete
// +kubebuilder:rbac:groups="",resources=configmaps,verbs=get;list;watch;create;update;patch;delete
// +kubebuilder:rbac:groups=apps,resources=deployments,verbs=get;list;watch;create;update;patch;delete

func (r *BranchReconciler) Reconcile(ctx context.Context, req ctrl.Request) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	log.Info("Reconcile loop start", "request", req)
	defer func() {
		log.Info("Reconcile loop end", "request", req)
	}()

	branch, err := r.getBranch(ctx, req)
	if err != nil || branch == nil {
		return ctrl.Result{}, err
	}

	ctx = context.WithValue(ctx, utils.BranchNameKey, branch.Name)

	result, err := r.reconcile(ctx, branch)
	if errors.Is(err, ErrRequeueAfterChange) {
		return result, nil
	} else if err != nil {
		log.Error(err, "Reconcile failed")
		return ctrl.Result{}, err
	}

	return result, nil
}

func (r *BranchReconciler) getBranch(ctx context.Context, req ctrl.Request) (*neonv1alpha1.Branch, error) {
	log := logf.FromContext(ctx)
	branch := &neonv1alpha1.Branch{}
	if err := r.Get(ctx, req.NamespacedName, branch); err != nil {
		if apierrors.IsNotFound(err) {
			log.Info("Branch has been deleted")
			return nil, nil
		}

		return nil, fmt.Errorf("cannot get the resource: %w", err)
	}
	return branch, nil
}

func (r *BranchReconciler) reconcile(ctx context.Context, branch *neonv1alpha1.Branch) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	if branch.Status.Phase == "" {
		if err := utils.SetPhases(ctx, r.Client, branch, utils.SetBranchCreatingStatus); err != nil {
			return ctrl.Result{}, fmt.Errorf("error setting default Status: %w", err)
		}
		log.Info("Branch phase set to creating")
	}

	// Generate timeline_id if not set
	if branch.Spec.TimelineID == "" {
		if err := r.updateTimelineID(ctx, branch); err != nil {
			return ctrl.Result{}, fmt.Errorf("failed to update timelineID: %w", err)
		}
		return ctrl.Result{RequeueAfter: time.Second}, nil
	}

	// Get the project for this branch
	project, err := r.getProject(ctx, branch.Spec.ProjectID, branch.Namespace)
	if err != nil {
		log.Error(err, "failed to get project", "projectID", branch.Spec.ProjectID)
		return ctrl.Result{}, err
	}

	// Create timeline on storage controller
	if err := r.ensureTimeline(ctx, branch, project); err != nil {
		log.Error(err, "failed to ensure timeline")
		return ctrl.Result{RequeueAfter: 10 * time.Second}, nil
	}

	// Create branch resources
	if err := r.createBranchResources(ctx, branch, project); err != nil {
		log.Error(err, "error while creating branch resources")
		if setErr := utils.SetPhases(ctx, r.Client, branch, utils.SetBranchCannotCreateResourcesStatus); setErr != nil {
			log.Error(setErr, "failed to set branch status")
		}
		return ctrl.Result{}, fmt.Errorf("not able to create branch resources: %w", err)
	}

	// Set branch to ready status after successful resource creation
	if err := utils.SetPhases(ctx, r.Client, branch, utils.SetBranchReadyStatus); err != nil {
		log.Error(err, "failed to set branch ready status")
		return ctrl.Result{}, fmt.Errorf("failed to update branch status to ready: %w", err)
	}
	log.Info("Branch status set to ready")

	return ctrl.Result{}, nil
}

func (r *BranchReconciler) updateTimelineID(ctx context.Context, branch *neonv1alpha1.Branch) error {
	log := logf.FromContext(ctx)

	current := &neonv1alpha1.Branch{}
	if err := r.Get(ctx, types.NamespacedName{Name: branch.GetName(), Namespace: branch.GetNamespace()}, current); err != nil {
		return err
	}

	timelineID := utils.GenerateNeonID()
	updated := current.DeepCopy()
	updated.Spec.TimelineID = timelineID
	updated.ManagedFields = nil

	if err := r.Patch(ctx, updated, client.MergeFrom(current), &client.PatchOptions{FieldManager: "neon-operator"}); err != nil {
		return err
	}

	branch.Spec.TimelineID = timelineID
	log.Info("Generated and set timelineID", "timelineID", timelineID)
	return nil
}

func (r *BranchReconciler) getProject(ctx context.Context, projectID string, namespace string) (*neonv1alpha1.Project, error) {
	project := &neonv1alpha1.Project{}
	namespacedName := types.NamespacedName{
		Name:      projectID,
		Namespace: namespace,
	}

	if err := r.Get(ctx, namespacedName, project); err != nil {
		return nil, fmt.Errorf("failed to get project %s: %w", projectID, err)
	}

	return project, nil
}

func (r *BranchReconciler) getCluster(ctx context.Context, clusterName, namespace string) (*neonv1alpha1.Cluster, error) {
	cluster := &neonv1alpha1.Cluster{}
	namespacedName := types.NamespacedName{
		Name:      clusterName,
		Namespace: namespace,
	}

	if err := r.Get(ctx, namespacedName, cluster); err != nil {
		return nil, fmt.Errorf("failed to get cluster %s: %w", clusterName, err)
	}

	return cluster, nil
}

func (r *BranchReconciler) ensureTimeline(ctx context.Context, branch *neonv1alpha1.Branch, project *neonv1alpha1.Project) error {
	log := logf.FromContext(ctx)

	pageserverURL := fmt.Sprintf(
		"http://%s-storage-controller:8080/v1/tenant/%s/timeline",
		project.Spec.ClusterName,
		project.Spec.TenantID,
	)

	log.Info("Sending request to pageserver", "url", pageserverURL)

	requestBody := map[string]interface{}{
		"new_timeline_id": branch.Spec.TimelineID,
		"pg_version":      branch.Spec.PGVersion,
	}

	bodyBytes, err := json.Marshal(requestBody)
	if err != nil {
		return fmt.Errorf("failed to marshal request body: %w", err)
	}

	httpClient := &http.Client{
		Timeout: 10 * time.Second,
	}

	resp, err := httpClient.Post(pageserverURL, "application/json", bytes.NewBuffer(bodyBytes))
	if err != nil {
		log.Info("Failed to connect to pageserver, will retry", "url", pageserverURL, "error", err)
		return fmt.Errorf("failed to connect to pageserver: %w", err)
	}
	defer func() {
		if err := resp.Body.Close(); err != nil {
			log.Error(err, "failed to close response body")
		}
	}()

	if resp.StatusCode != http.StatusOK && resp.StatusCode != http.StatusCreated && resp.StatusCode != http.StatusConflict {
		log.Info("Failed to create timeline on pageserver", "status", resp.StatusCode)
		return fmt.Errorf("failed to create timeline on pageserver, status: %d", resp.StatusCode)
	}

	log.Info("Successfully created timeline on pageserver")
	return nil
}

// Resource creation functions moved to branch_create.go

// SetupWithManager sets up the controller with the Manager.
func (r *BranchReconciler) SetupWithManager(mgr ctrl.Manager) error {
	return ctrl.NewControllerManagedBy(mgr).
		For(&neonv1alpha1.Branch{}).
		Owns(&appsv1.Deployment{}).
		Owns(&corev1.Service{}).
		Owns(&corev1.ConfigMap{}).
		Named("branch").
		Complete(r)
}
