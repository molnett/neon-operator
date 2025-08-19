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
	"errors"
	"fmt"
	"net/http"
	"time"

	apierrors "k8s.io/apimachinery/pkg/api/errors"
	"k8s.io/apimachinery/pkg/runtime"
	"k8s.io/apimachinery/pkg/types"
	ctrl "sigs.k8s.io/controller-runtime"
	"sigs.k8s.io/controller-runtime/pkg/client"
	logf "sigs.k8s.io/controller-runtime/pkg/log"

	neonv1alpha1 "oltp.molnett.org/neon-operator/api/v1alpha1"
	"oltp.molnett.org/neon-operator/utils"
)

// ProjectReconciler reconciles a Project object
type ProjectReconciler struct {
	client.Client
	Scheme *runtime.Scheme
}

// +kubebuilder:rbac:groups=neon.oltp.molnett.org,resources=projects,verbs=get;list;watch;create;update;patch;delete
// +kubebuilder:rbac:groups=neon.oltp.molnett.org,resources=projects/status,verbs=get;update;patch
// +kubebuilder:rbac:groups=neon.oltp.molnett.org,resources=projects/finalizers,verbs=update

func (r *ProjectReconciler) Reconcile(ctx context.Context, req ctrl.Request) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	log.Info("Reconcile loop start", "request", req)
	defer func() {
		log.Info("Reconcile loop end", "request", req)
	}()

	project, err := r.getProject(ctx, req)
	if err != nil {
		return ctrl.Result{}, err
	}

	if project == nil {
		return ctrl.Result{}, nil
	}

	ctx = context.WithValue(ctx, utils.ProjectNameKey, project.Name)

	result, err := r.reconcile(ctx, project)
	if errors.Is(err, ErrRequeueAfterChange) {
		return result, nil
	} else if err != nil {
		log.Error(err, "Reconcile failed")
		return ctrl.Result{}, err
	}

	return result, nil
}

func (r *ProjectReconciler) reconcile(ctx context.Context, project *neonv1alpha1.Project) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	if project.Status.Phase == "" {
		if err := utils.SetPhases(ctx, r.Client, project, utils.SetProjectPendingStatus); err != nil {
			return ctrl.Result{}, fmt.Errorf("error setting pending status: %w", err)
		}
		log.Info("Project phase set to pending")
	}

	if project.Spec.TenantID == "" {
		tenantID := utils.GenerateNeonID()

		if err := utils.SetPhases(ctx, r.Client, project, utils.SetProjectCreatingStatus); err != nil {
			return ctrl.Result{}, fmt.Errorf("error setting creating status: %w", err)
		}

		if err := r.updateTenantID(ctx, project, tenantID); err != nil {
			log.Error(err, "Failed to update tenant ID")
			utils.SetPhases(ctx, r.Client, project, func(p *neonv1alpha1.Project) {
				utils.SetProjectTenantCreationFailedStatus(p, fmt.Sprintf("Failed to update tenant ID: %v", err))
			})
			return ctrl.Result{}, fmt.Errorf("failed to update tenant ID: %w", err)
		}

		log.Info("Generated and set tenant ID", "tenantID", tenantID)
		return ctrl.Result{RequeueAfter: time.Second}, ErrRequeueAfterChange
	}

	err := r.ensureTenantOnPageserver(ctx, project)
	if err != nil {
		log.Error(err, "Failed to ensure tenant on pageserver")
		return ctrl.Result{RequeueAfter: 10 * time.Second}, nil
	}

	if err := utils.SetPhases(ctx, r.Client, project, utils.SetProjectReadyStatus); err != nil {
		return ctrl.Result{}, fmt.Errorf("error setting ready status: %w", err)
	}
	log.Info("Project is ready")

	return ctrl.Result{}, nil
}

func (r *ProjectReconciler) getProject(ctx context.Context, req ctrl.Request) (*neonv1alpha1.Project, error) {
	log := logf.FromContext(ctx)
	project := &neonv1alpha1.Project{}
	if err := r.Get(ctx, req.NamespacedName, project); err != nil {
		if apierrors.IsNotFound(err) {
			log.Info("Project has been deleted")
			return nil, nil
		}

		return nil, fmt.Errorf("cannot get the resource: %w", err)
	}
	return project, nil
}

func (r *ProjectReconciler) updateTenantID(ctx context.Context, project *neonv1alpha1.Project, tenantID string) error {
	current := &neonv1alpha1.Project{}
	if err := r.Get(ctx, types.NamespacedName{Name: project.GetName(), Namespace: project.GetNamespace()}, current); err != nil {
		return err
	}

	updated := current.DeepCopy()
	updated.Spec.TenantID = tenantID
	updated.ManagedFields = nil

	if err := r.Patch(ctx, updated, client.Apply, &client.PatchOptions{FieldManager: "neon-operator"}); err != nil {
		return err
	}

	project.Spec.TenantID = tenantID
	return nil
}

func (r *ProjectReconciler) ensureTenantOnPageserver(ctx context.Context, project *neonv1alpha1.Project) error {
	log := logf.FromContext(ctx)

	storageControllerURL := fmt.Sprintf(
		"http://%s-storage-controller:8080/v1/tenant/%s/location_config",
		project.Spec.ClusterName,
		project.Spec.TenantID,
	)

	log.Info("Sending request to pageserver", "url", storageControllerURL)

	requestBody := []byte(`{"mode": "AttachedSingle", "generation": 1, "tenant_conf": {}}`)
	req, err := http.NewRequestWithContext(ctx, http.MethodPut, storageControllerURL, bytes.NewBuffer(requestBody))
	if err != nil {
		return fmt.Errorf("failed to create request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")

	client := &http.Client{Timeout: 30 * time.Second}
	resp, err := client.Do(req)
	if err != nil {
		log.Info("Failed to connect to pageserver, will retry", "error", err, "url", storageControllerURL)
		utils.SetPhases(ctx, r.Client, project, func(p *neonv1alpha1.Project) {
			utils.SetProjectPageserverConnectionErrorStatus(p, fmt.Sprintf("Failed to connect to pageserver: %v", err))
		})
		return err
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		log.Info("Pageserver returned error status", "status", resp.Status)
		utils.SetPhases(ctx, r.Client, project, func(p *neonv1alpha1.Project) {
			utils.SetProjectTenantCreationFailedStatus(p, fmt.Sprintf("Pageserver returned status: %s", resp.Status))
		})
		return fmt.Errorf("pageserver returned status: %s", resp.Status)
	}

	log.Info("Successfully created tenant on pageserver")
	return nil
}

// SetupWithManager sets up the controller with the Manager.
func (r *ProjectReconciler) SetupWithManager(mgr ctrl.Manager) error {
	return ctrl.NewControllerManagedBy(mgr).
		For(&neonv1alpha1.Project{}).
		Named("project").
		Complete(r)
}
