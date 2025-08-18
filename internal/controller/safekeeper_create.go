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
	"oltp.molnett.org/neon-operator/specs/safekeeper"
	"oltp.molnett.org/neon-operator/utils"
)

func (r *SafekeeperReconciler) createSafekeeperResources(ctx context.Context, sk *neonv1alpha1.Safekeeper) error {
	log := logf.FromContext(ctx)

	log.Info("Reconciling safekeeper PVC")
	if err := r.reconcilePVC(ctx, sk); err != nil {
		return err
	}

	log.Info("Reconciling safekeeper Pod")
	if err := r.reconcilePod(ctx, sk); err != nil {
		return err
	}

	log.Info("Reconciling safekeeper Service")
	if err := r.reconcileService(ctx, sk); err != nil {
		return err
	}

	return nil
}

func (r *SafekeeperReconciler) reconcilePVC(ctx context.Context, sk *neonv1alpha1.Safekeeper) error {
	log := logf.FromContext(ctx)

	intendedPVC := safekeeper.PersistentVolumeClaim(sk)

	var currentPVC corev1.PersistentVolumeClaim
	getErr := r.Client.Get(ctx, types.NamespacedName{Name: intendedPVC.Name, Namespace: sk.Namespace}, &currentPVC)
	if getErr != nil && !apierrors.IsNotFound(getErr) {
		return fmt.Errorf("failed to get safekeeper PVC: %w", getErr)
	}

	err := ctrl.SetControllerReference(sk, intendedPVC, r.Scheme)
	if err != nil {
		return fmt.Errorf("failed to set controller reference for safekeeper PVC: %w", err)
	}

	// If PVC does not exist, create it
	if apierrors.IsNotFound(getErr) {
		if err := r.Client.Create(ctx, intendedPVC, &client.CreateOptions{
			FieldManager: utils.FieldManager,
		}); err != nil {
			return fmt.Errorf("failed to create safekeeper PVC: %w", err)
		}
		log.Info("Safekeeper PVC created", "name", sk.Name)
		return nil
	}

	// PVC exists - no updates needed for storage resources
	return nil
}

func (r *SafekeeperReconciler) reconcilePod(ctx context.Context, sk *neonv1alpha1.Safekeeper) error {
	log := logf.FromContext(ctx)

	// Get the parent cluster to get the image
	var cluster neonv1alpha1.Cluster
	if err := r.Client.Get(ctx, types.NamespacedName{Name: sk.Spec.Cluster, Namespace: sk.Namespace}, &cluster); err != nil {
		return fmt.Errorf("failed to get parent cluster: %w", err)
	}

	intendedPod := safekeeper.Pod(sk, cluster.Spec.NeonImage)

	var currentPod corev1.Pod
	getErr := r.Client.Get(ctx, types.NamespacedName{Name: intendedPod.Name, Namespace: sk.Namespace}, &currentPod)
	if getErr != nil && !apierrors.IsNotFound(getErr) {
		return fmt.Errorf("failed to get safekeeper pod: %w", getErr)
	}

	err := ctrl.SetControllerReference(sk, intendedPod, r.Scheme)
	if err != nil {
		return fmt.Errorf("failed to set controller reference for safekeeper pod: %w", err)
	}

	// If pod does not exist, create it
	if apierrors.IsNotFound(getErr) {
		if err := r.Client.Create(ctx, intendedPod, &client.CreateOptions{
			FieldManager: utils.FieldManager,
		}); err != nil {
			return fmt.Errorf("failed to create safekeeper pod: %w", err)
		}
		log.Info("Safekeeper pod created", "name", sk.Name)
		return nil
	}

	// Use DeepDerivative with correct order: intended is subset of current
	if !equality.Semantic.DeepDerivative(intendedPod.Spec, currentPod.Spec) {
		// At this point, the pod exists and needs to be updated
		if err := r.Client.Patch(ctx, intendedPod, client.Apply, &client.PatchOptions{
			Force:        ptr.To(true),
			FieldManager: utils.FieldManager,
		}); err != nil {
			return fmt.Errorf("failed to update safekeeper pod: %w", err)
		}
		log.Info("Safekeeper pod updated", "name", sk.Name)
		return nil
	}

	return nil
}

func (r *SafekeeperReconciler) reconcileService(ctx context.Context, sk *neonv1alpha1.Safekeeper) error {
	log := logf.FromContext(ctx)

	intendedService := safekeeper.Service(sk)

	var currentService corev1.Service
	getErr := r.Client.Get(ctx, types.NamespacedName{Name: intendedService.Name, Namespace: sk.Namespace}, &currentService)
	if getErr != nil && !apierrors.IsNotFound(getErr) {
		return fmt.Errorf("failed to get safekeeper service: %w", getErr)
	}

	err := ctrl.SetControllerReference(sk, intendedService, r.Scheme)
	if err != nil {
		return fmt.Errorf("failed to set controller reference for safekeeper service: %w", err)
	}

	// If service does not exist, create it
	if apierrors.IsNotFound(getErr) {
		if err := r.Client.Create(ctx, intendedService, &client.CreateOptions{
			FieldManager: utils.FieldManager,
		}); err != nil {
			return fmt.Errorf("failed to create safekeeper service: %w", err)
		}
		log.Info("Safekeeper service created", "name", sk.Name)
		return nil
	}

	// Use DeepDerivative with correct order: intended is subset of current
	if !equality.Semantic.DeepDerivative(intendedService.Spec, currentService.Spec) {
		// At this point, the service exists and needs to be updated
		if err := r.Client.Patch(ctx, intendedService, client.Apply, &client.PatchOptions{
			Force:        ptr.To(true),
			FieldManager: utils.FieldManager,
		}); err != nil {
			return fmt.Errorf("failed to update safekeeper service: %w", err)
		}
		log.Info("Safekeeper service updated", "name", sk.Name)
		return nil
	}

	return nil
}
