package controller

import (
	"context"
	"fmt"

	appsv1 "k8s.io/api/apps/v1"
	corev1 "k8s.io/api/core/v1"
	"k8s.io/apimachinery/pkg/api/equality"
	apierrors "k8s.io/apimachinery/pkg/api/errors"
	"k8s.io/apimachinery/pkg/types"
	"k8s.io/utils/ptr"
	"sigs.k8s.io/controller-runtime/pkg/client"
	ctrl "sigs.k8s.io/controller-runtime/pkg/controller/controllerutil"
	logf "sigs.k8s.io/controller-runtime/pkg/log"

	neonv1alpha1 "oltp.molnett.org/neon-operator/api/v1alpha1"
	"oltp.molnett.org/neon-operator/specs/compute"
	"oltp.molnett.org/neon-operator/utils"
)

func (r *BranchReconciler) createBranchResources(ctx context.Context, branch *neonv1alpha1.Branch, project *neonv1alpha1.Project) error {
	log := logf.FromContext(ctx)

	log.Info("Reconciling branch ConfigMap")
	if err := r.reconcileConfigMap(ctx, branch, project); err != nil {
		return err
	}

	log.Info("Reconciling branch Deployment")
	if err := r.reconcileDeployment(ctx, branch, project); err != nil {
		return err
	}

	log.Info("Reconciling branch admin Service")
	if err := r.reconcileAdminService(ctx, branch, project); err != nil {
		return err
	}

	log.Info("Reconciling branch postgres Service")
	if err := r.reconcilePostgresService(ctx, branch, project); err != nil {
		return err
	}

	return nil
}

func (r *BranchReconciler) reconcileConfigMap(ctx context.Context, branch *neonv1alpha1.Branch, project *neonv1alpha1.Project) error {
	log := logf.FromContext(ctx)

	cluster, err := r.getCluster(ctx, project.Spec.ClusterName, "neon")
	if err != nil {
		return err
	}

	var jwkSecret corev1.Secret
	err = r.Get(ctx, client.ObjectKey{Name: fmt.Sprintf("cluster-%s-jwt", cluster.Name), Namespace: cluster.Namespace}, &jwkSecret)
	if err != nil {
		return err
	}

	intendedConfigMap, err := compute.ConfigMap(branch, project, jwkSecret)
	if err != nil {
		return err
	}

	var currentConfigMap corev1.ConfigMap
	getErr := r.Get(ctx, types.NamespacedName{Name: intendedConfigMap.Name, Namespace: branch.Namespace}, &currentConfigMap)
	if getErr != nil && !apierrors.IsNotFound(getErr) {
		return fmt.Errorf("failed to get branch ConfigMap: %w", getErr)
	}

	err = ctrl.SetControllerReference(branch, intendedConfigMap, r.Scheme)
	if err != nil {
		return fmt.Errorf("failed to set controller reference for branch ConfigMap: %w", err)
	}

	// If ConfigMap does not exist, create it
	if apierrors.IsNotFound(getErr) {
		if err := r.Create(ctx, intendedConfigMap, &client.CreateOptions{
			FieldManager: utils.FieldManager,
		}); err != nil {
			return fmt.Errorf("failed to create branch ConfigMap: %w", err)
		}
		log.Info("Branch ConfigMap created", "name", branch.Name)
		return nil
	}

	// Use DeepDerivative with correct order: intended is subset of current
	if !equality.Semantic.DeepDerivative(intendedConfigMap.Data, currentConfigMap.Data) {
		// At this point, the ConfigMap exists and needs to be updated
		if err := r.Patch(ctx, intendedConfigMap, client.Apply, &client.PatchOptions{
			Force:        ptr.To(true),
			FieldManager: utils.FieldManager,
		}); err != nil {
			return fmt.Errorf("failed to update branch ConfigMap: %w", err)
		}
		log.Info("Branch ConfigMap updated", "name", branch.Name)
		return nil
	}

	return nil
}

func (r *BranchReconciler) reconcileDeployment(ctx context.Context, branch *neonv1alpha1.Branch, project *neonv1alpha1.Project) error {
	log := logf.FromContext(ctx)

	intendedDeployment := compute.Deployment(branch, project)

	var currentDeployment appsv1.Deployment
	getErr := r.Get(ctx, types.NamespacedName{Name: intendedDeployment.Name, Namespace: branch.Namespace}, &currentDeployment)
	if getErr != nil && !apierrors.IsNotFound(getErr) {
		return fmt.Errorf("failed to get branch Deployment: %w", getErr)
	}

	err := ctrl.SetControllerReference(branch, intendedDeployment, r.Scheme)
	if err != nil {
		return fmt.Errorf("failed to set controller reference for branch Deployment: %w", err)
	}

	// If Deployment does not exist, create it
	if apierrors.IsNotFound(getErr) {
		if err := r.Create(ctx, intendedDeployment, &client.CreateOptions{
			FieldManager: utils.FieldManager,
		}); err != nil {
			return fmt.Errorf("failed to create branch Deployment: %w", err)
		}
		log.Info("Branch Deployment created", "name", branch.Name)
		return nil
	}

	if !equality.Semantic.DeepDerivative(intendedDeployment.Spec, currentDeployment.Spec) {
		// At this point, the Deployment exists and needs to be updated
		if err := r.Patch(ctx, intendedDeployment, client.Apply, &client.PatchOptions{
			Force:        ptr.To(true),
			FieldManager: utils.FieldManager,
		}); err != nil {
			return fmt.Errorf("failed to update branch Deployment: %w", err)
		}
		log.Info("Branch Deployment updated", "name", branch.Name)
		return nil
	}

	return nil
}

func (r *BranchReconciler) reconcileAdminService(ctx context.Context, branch *neonv1alpha1.Branch, project *neonv1alpha1.Project) error {
	log := logf.FromContext(ctx)

	intendedService := compute.AdminService(branch, project)

	var currentService corev1.Service
	getErr := r.Get(ctx, types.NamespacedName{Name: intendedService.Name, Namespace: branch.Namespace}, &currentService)
	if getErr != nil && !apierrors.IsNotFound(getErr) {
		return fmt.Errorf("failed to get branch admin Service: %w", getErr)
	}

	err := ctrl.SetControllerReference(branch, intendedService, r.Scheme)
	if err != nil {
		return fmt.Errorf("failed to set controller reference for branch admin Service: %w", err)
	}

	// If Service does not exist, create it
	if apierrors.IsNotFound(getErr) {
		if err := r.Create(ctx, intendedService, &client.CreateOptions{
			FieldManager: utils.FieldManager,
		}); err != nil {
			return fmt.Errorf("failed to create branch admin Service: %w", err)
		}
		log.Info("Branch admin Service created", "name", branch.Name)
		return nil
	}

	// Use DeepDerivative with correct order: intended is subset of current
	if !equality.Semantic.DeepDerivative(intendedService.Spec, currentService.Spec) {
		// At this point, the Service exists and needs to be updated
		if err := r.Patch(ctx, intendedService, client.Apply, &client.PatchOptions{
			Force:        ptr.To(true),
			FieldManager: utils.FieldManager,
		}); err != nil {
			return fmt.Errorf("failed to update branch admin Service: %w", err)
		}
		log.Info("Branch admin Service updated", "name", branch.Name)
		return nil
	}

	return nil
}

func (r *BranchReconciler) reconcilePostgresService(ctx context.Context, branch *neonv1alpha1.Branch, project *neonv1alpha1.Project) error {
	log := logf.FromContext(ctx)

	intendedService := compute.PostgresService(branch, project)

	var currentService corev1.Service
	getErr := r.Get(ctx, types.NamespacedName{Name: intendedService.Name, Namespace: branch.Namespace}, &currentService)
	if getErr != nil && !apierrors.IsNotFound(getErr) {
		return fmt.Errorf("failed to get branch postgres Service: %w", getErr)
	}

	err := ctrl.SetControllerReference(branch, intendedService, r.Scheme)
	if err != nil {
		return fmt.Errorf("failed to set controller reference for branch postgres Service: %w", err)
	}

	// If Service does not exist, create it
	if apierrors.IsNotFound(getErr) {
		if err := r.Create(ctx, intendedService, &client.CreateOptions{
			FieldManager: utils.FieldManager,
		}); err != nil {
			return fmt.Errorf("failed to create branch postgres Service: %w", err)
		}
		log.Info("Branch postgres Service created", "name", branch.Name)
		return nil
	}

	// Use DeepDerivative with correct order: intended is subset of current
	if !equality.Semantic.DeepDerivative(intendedService.Spec, currentService.Spec) {
		// At this point, the Service exists and needs to be updated
		if err := r.Patch(ctx, intendedService, client.Apply, &client.PatchOptions{
			Force:        ptr.To(true),
			FieldManager: utils.FieldManager,
		}); err != nil {
			return fmt.Errorf("failed to update branch postgres Service: %w", err)
		}
		log.Info("Branch postgres Service updated", "name", branch.Name)
		return nil
	}

	return nil
}
