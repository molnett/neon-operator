package controller

import (
	"context"
	"fmt"

	corev1 "k8s.io/api/core/v1"
	"k8s.io/apimachinery/pkg/api/equality"
	apierrors "k8s.io/apimachinery/pkg/api/errors"
	"k8s.io/apimachinery/pkg/types"
	"k8s.io/utils/ptr"
	"sigs.k8s.io/controller-runtime/pkg/client"
	ctrl "sigs.k8s.io/controller-runtime/pkg/controller/controllerutil"
	logf "sigs.k8s.io/controller-runtime/pkg/log"

	neonv1alpha1 "oltp.molnett.org/neon-operator/api/v1alpha1"
	"oltp.molnett.org/neon-operator/specs/pageserver"
	"oltp.molnett.org/neon-operator/utils"
)

func (r *PageserverReconciler) createPageserverResources(ctx context.Context, ps *neonv1alpha1.Pageserver) error {
	log := logf.FromContext(ctx)

	log.Info("Reconciling pageserver ConfigMap")
	if err := r.reconcileConfigMap(ctx, ps); err != nil {
		return err
	}

	log.Info("Reconciling pageserver PVC")
	if err := r.reconcilePVC(ctx, ps); err != nil {
		return err
	}

	log.Info("Reconciling pageserver Pod")
	if err := r.reconcilePod(ctx, ps); err != nil {
		return err
	}

	log.Info("Reconciling pageserver Service")
	if err := r.reconcileService(ctx, ps); err != nil {
		return err
	}

	return nil
}

func (r *PageserverReconciler) reconcileConfigMap(ctx context.Context, ps *neonv1alpha1.Pageserver) error {
	log := logf.FromContext(ctx)

	// Get the bucket credentials secret
	var bucketSecret corev1.Secret
	if err := r.Client.Get(ctx, types.NamespacedName{Name: ps.Spec.BucketCredentialsSecret.Name, Namespace: ps.Namespace}, &bucketSecret); err != nil {
		return fmt.Errorf("failed to get bucket credentials secret: %w", err)
	}

	intendedConfigMap := pageserver.ConfigMap(ps, &bucketSecret)

	var currentConfigMap corev1.ConfigMap
	getErr := r.Client.Get(ctx, types.NamespacedName{Name: intendedConfigMap.Name, Namespace: ps.Namespace}, &currentConfigMap)
	if getErr != nil && !apierrors.IsNotFound(getErr) {
		return fmt.Errorf("failed to get pageserver ConfigMap: %w", getErr)
	}

	err := ctrl.SetControllerReference(ps, intendedConfigMap, r.Scheme)
	if err != nil {
		return fmt.Errorf("failed to set controller reference for pageserver ConfigMap: %w", err)
	}

	// If ConfigMap does not exist, create it
	if apierrors.IsNotFound(getErr) {
		if err := r.Client.Create(ctx, intendedConfigMap, &client.CreateOptions{
			FieldManager: utils.FieldManager,
		}); err != nil {
			return fmt.Errorf("failed to create pageserver ConfigMap: %w", err)
		}
		log.Info("Pageserver ConfigMap created", "name", ps.Name)
		return nil
	}

	// Use DeepDerivative with correct order: intended is subset of current
	if !equality.Semantic.DeepDerivative(intendedConfigMap.Data, currentConfigMap.Data) {
		// At this point, the ConfigMap exists and needs to be updated
		if err := r.Client.Patch(ctx, intendedConfigMap, client.Apply, &client.PatchOptions{
			Force:        ptr.To(true),
			FieldManager: utils.FieldManager,
		}); err != nil {
			return fmt.Errorf("failed to update pageserver ConfigMap: %w", err)
		}
		log.Info("Pageserver ConfigMap updated", "name", ps.Name)
		return nil
	}

	return nil
}

func (r *PageserverReconciler) reconcilePVC(ctx context.Context, ps *neonv1alpha1.Pageserver) error {
	log := logf.FromContext(ctx)

	intendedPVC := pageserver.PersistentVolumeClaim(ps)

	var currentPVC corev1.PersistentVolumeClaim
	getErr := r.Client.Get(ctx, types.NamespacedName{Name: intendedPVC.Name, Namespace: ps.Namespace}, &currentPVC)
	if getErr != nil && !apierrors.IsNotFound(getErr) {
		return fmt.Errorf("failed to get pageserver PVC: %w", getErr)
	}

	err := ctrl.SetControllerReference(ps, intendedPVC, r.Scheme)
	if err != nil {
		return fmt.Errorf("failed to set controller reference for pageserver PVC: %w", err)
	}

	// If PVC does not exist, create it
	if apierrors.IsNotFound(getErr) {
		if err := r.Client.Create(ctx, intendedPVC, &client.CreateOptions{
			FieldManager: utils.FieldManager,
		}); err != nil {
			return fmt.Errorf("failed to create pageserver PVC: %w", err)
		}
		log.Info("Pageserver PVC created", "name", ps.Name)
		return nil
	}

	// PVC exists - no updates needed for storage resources
	return nil
}

func (r *PageserverReconciler) reconcilePod(ctx context.Context, ps *neonv1alpha1.Pageserver) error {
	log := logf.FromContext(ctx)

	// Get the parent cluster to get the image
	var cluster neonv1alpha1.Cluster
	if err := r.Client.Get(ctx, types.NamespacedName{Name: ps.Spec.Cluster, Namespace: ps.Namespace}, &cluster); err != nil {
		return fmt.Errorf("failed to get parent cluster: %w", err)
	}

	intendedPod := pageserver.Pod(ps, cluster.Spec.NeonImage)

	var currentPod corev1.Pod
	getErr := r.Client.Get(ctx, types.NamespacedName{Name: intendedPod.Name, Namespace: ps.Namespace}, &currentPod)
	if getErr != nil && !apierrors.IsNotFound(getErr) {
		return fmt.Errorf("failed to get pageserver pod: %w", getErr)
	}

	err := ctrl.SetControllerReference(ps, intendedPod, r.Scheme)
	if err != nil {
		return fmt.Errorf("failed to set controller reference for pageserver pod: %w", err)
	}

	// If pod does not exist, create it
	if apierrors.IsNotFound(getErr) {
		if err := r.Client.Create(ctx, intendedPod, &client.CreateOptions{
			FieldManager: utils.FieldManager,
		}); err != nil {
			return fmt.Errorf("failed to create pageserver pod: %w", err)
		}
		log.Info("Pageserver pod created", "name", ps.Name)
		return nil
	}

	// Use DeepDerivative with correct order: intended is subset of current
	if !equality.Semantic.DeepDerivative(intendedPod.Spec, currentPod.Spec) {
		// At this point, the pod exists and needs to be updated
		if err := r.Client.Patch(ctx, intendedPod, client.Apply, &client.PatchOptions{
			Force:        ptr.To(true),
			FieldManager: utils.FieldManager,
		}); err != nil {
			return fmt.Errorf("failed to update pageserver pod: %w", err)
		}
		log.Info("Pageserver pod updated", "name", ps.Name)
		return nil
	}

	return nil
}

func (r *PageserverReconciler) reconcileService(ctx context.Context, ps *neonv1alpha1.Pageserver) error {
	log := logf.FromContext(ctx)

	intendedService := pageserver.Service(ps)

	var currentService corev1.Service
	getErr := r.Client.Get(ctx, types.NamespacedName{Name: intendedService.Name, Namespace: ps.Namespace}, &currentService)
	if getErr != nil && !apierrors.IsNotFound(getErr) {
		return fmt.Errorf("failed to get pageserver service: %w", getErr)
	}

	err := ctrl.SetControllerReference(ps, intendedService, r.Scheme)
	if err != nil {
		return fmt.Errorf("failed to set controller reference for pageserver service: %w", err)
	}

	// If service does not exist, create it
	if apierrors.IsNotFound(getErr) {
		if err := r.Client.Create(ctx, intendedService, &client.CreateOptions{
			FieldManager: utils.FieldManager,
		}); err != nil {
			return fmt.Errorf("failed to create pageserver service: %w", err)
		}
		log.Info("Pageserver service created", "name", ps.Name)
		return nil
	}

	// Use DeepDerivative with correct order: intended is subset of current
	if !equality.Semantic.DeepDerivative(intendedService.Spec, currentService.Spec) {
		// At this point, the service exists and needs to be updated
		if err := r.Client.Patch(ctx, intendedService, client.Apply, &client.PatchOptions{
			Force:        ptr.To(true),
			FieldManager: utils.FieldManager,
		}); err != nil {
			return fmt.Errorf("failed to update pageserver service: %w", err)
		}
		log.Info("Pageserver service updated", "name", ps.Name)
		return nil
	}

	return nil
}
