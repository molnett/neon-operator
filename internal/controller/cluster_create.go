package controller

import (
	"context"
	"crypto/ed25519"
	"crypto/rand"
	"crypto/x509"
	"encoding/pem"
	"fmt"

	appsv1 "k8s.io/api/apps/v1"
	corev1 "k8s.io/api/core/v1"
	"k8s.io/apimachinery/pkg/api/equality"
	apierrors "k8s.io/apimachinery/pkg/api/errors"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/apimachinery/pkg/types"
	"k8s.io/utils/ptr"
	"sigs.k8s.io/controller-runtime/pkg/client"
	ctrl "sigs.k8s.io/controller-runtime/pkg/controller/controllerutil"
	logf "sigs.k8s.io/controller-runtime/pkg/log"

	neonv1alpha1 "oltp.molnett.org/neon-operator/api/v1alpha1"
	"oltp.molnett.org/neon-operator/specs/storagebroker"
	"oltp.molnett.org/neon-operator/specs/storagecontroller"
	"oltp.molnett.org/neon-operator/utils"
)

func (r *ClusterReconciler) createClusterResources(ctx context.Context, cluster *neonv1alpha1.Cluster) error {
	log := logf.FromContext(ctx)

	log.Info("Reconciling JWT keys")

	if err := r.reconcileJWTKeys(ctx, cluster); err != nil {
		return err
	}

	log.Info("Reconciling storage controller")

	if err := r.reconcileStorageController(ctx, cluster); err != nil {
		return err
	}

	log.Info("Reconciling storage broker")

	if err := r.reconcileStorageBroker(ctx, cluster); err != nil {
		return err
	}

	return nil
}

func (r *ClusterReconciler) reconcileJWTKeys(ctx context.Context, cluster *neonv1alpha1.Cluster) error {
	log := logf.FromContext(ctx)

	var secret corev1.Secret
	err := r.Get(ctx, client.ObjectKey{Name: fmt.Sprintf("cluster-%s-jwt", cluster.Name), Namespace: cluster.Namespace}, &secret)
	if err == nil {
		return nil
	} else if !apierrors.IsNotFound(err) {
		return err
	}
	log.Info("Creating new JWT keys secret for cluster", "cluster", cluster.Name)

	pub, priv, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		return err
	}

	privKeyVBytes, err := x509.MarshalPKCS8PrivateKey(priv)
	if err != nil {
		return err
	}

	privKeyPEM := pem.EncodeToMemory(&pem.Block{
		Type:  "PRIVATE KEY",
		Bytes: privKeyVBytes,
	})

	pubKeyVBytes, err := x509.MarshalPKIXPublicKey(pub)
	if err != nil {
		return err
	}

	pubKeyPEM := pem.EncodeToMemory(&pem.Block{
		Type:  "PUBLIC KEY",
		Bytes: pubKeyVBytes,
	})

	jwtSecret := &corev1.Secret{
		ObjectMeta: metav1.ObjectMeta{
			Name:      fmt.Sprintf("cluster-%s-jwt", cluster.Name),
			Namespace: cluster.Namespace,
		},
		Data: map[string][]byte{
			"private.pem": privKeyPEM,
			"public.pem":  pubKeyPEM,
		},
	}

	if err := ctrl.SetControllerReference(cluster, jwtSecret, r.Scheme); err != nil {
		return fmt.Errorf("failed to set controller reference: %w", err)
	}

	if err := r.Create(ctx, jwtSecret); err != nil {
		return err
	}

	return nil
}

func (r *ClusterReconciler) reconcileStorageController(ctx context.Context, cluster *neonv1alpha1.Cluster) error {
	if err := r.reconcileStorageControllerDeployment(ctx, cluster); err != nil {
		return err
	}

	if err := r.reconcileStorageControllerService(ctx, cluster); err != nil {
		return err
	}

	return nil
}

func (r *ClusterReconciler) reconcileStorageBroker(ctx context.Context, cluster *neonv1alpha1.Cluster) error {
	if err := r.reconcileStorageBrokerDeployment(ctx, cluster); err != nil {
		return err
	}

	if err := r.reconcileStorageBrokerService(ctx, cluster); err != nil {
		return err
	}

	return nil
}

func (r *ClusterReconciler) reconcileStorageControllerDeployment(ctx context.Context, cluster *neonv1alpha1.Cluster) error {
	log := logf.FromContext(ctx)

	var databaseSecret corev1.Secret
	if err := r.Get(ctx, types.NamespacedName{Name: cluster.Spec.StorageControllerDatabaseSecret.Name, Namespace: cluster.Namespace}, &databaseSecret); err != nil {
		return fmt.Errorf("failed to get storage controller database secret: %w", err)
	}

	intendedDeployment := storagecontroller.Deployment(cluster)

	var currentDeployment appsv1.Deployment
	getErr := r.Get(ctx, types.NamespacedName{Name: intendedDeployment.Name, Namespace: cluster.Namespace}, &currentDeployment)
	if getErr != nil && !apierrors.IsNotFound(getErr) {
		return fmt.Errorf("failed to get storage controller deployment: %w", getErr)
	}

	err := ctrl.SetControllerReference(cluster, intendedDeployment, r.Scheme)
	if err != nil {
		return fmt.Errorf("failed to set controller reference for storage controller deployment: %w", err)
	}

	// If deployment does not exist, create it
	if apierrors.IsNotFound(getErr) {
		if err := r.Create(ctx, intendedDeployment, &client.CreateOptions{
			FieldManager: utils.FieldManager,
		}); err != nil {
			return fmt.Errorf("failed to create storage controller deployment: %w", err)
		}
		log.Info("Storage controller deployment created", "name", cluster.Name)
		return nil
	}

	// Use DeepDerivative with correct order: intended is subset of current
	if !equality.Semantic.DeepDerivative(intendedDeployment.Spec, currentDeployment.Spec) {
		// At this point, the deployment exists and needs to be updated
		if err := r.Patch(ctx, intendedDeployment, client.Apply, &client.PatchOptions{
			Force:        ptr.To(true),
			FieldManager: utils.FieldManager,
		}); err != nil {
			return fmt.Errorf("failed to update storage controller deployment: %w", err)
		}
		log.Info("Storage controller deployment updated", "name", cluster.Name)
		return nil
	}

	return nil
}

func (r *ClusterReconciler) reconcileStorageControllerService(ctx context.Context, cluster *neonv1alpha1.Cluster) error {
	log := logf.FromContext(ctx)

	intendedService := storagecontroller.Service(cluster)

	var currentService corev1.Service
	getErr := r.Get(ctx, types.NamespacedName{Name: intendedService.Name, Namespace: cluster.Namespace}, &currentService)
	if getErr != nil && !apierrors.IsNotFound(getErr) {
		return fmt.Errorf("failed to get storage controller service: %w", getErr)
	}

	err := ctrl.SetControllerReference(cluster, intendedService, r.Scheme)
	if err != nil {
		return fmt.Errorf("failed to set controller reference for storage controller service: %w", err)
	}

	// If service does not exist, create it
	if apierrors.IsNotFound(getErr) {
		if err := r.Create(ctx, intendedService, &client.CreateOptions{
			FieldManager: utils.FieldManager,
		}); err != nil {
			return fmt.Errorf("failed to create storage controller service: %w", err)
		}
		log.Info("Storage controller service created", "name", cluster.Name)
		return nil
	}

	// Use DeepDerivative with correct order: intended is subset of current
	if !equality.Semantic.DeepDerivative(intendedService.Spec, currentService.Spec) {
		// At this point, the service exists and needs to be updated
		if err := r.Patch(ctx, intendedService, client.Apply, &client.PatchOptions{
			Force:        ptr.To(true),
			FieldManager: utils.FieldManager,
		}); err != nil {
			return fmt.Errorf("failed to update storage controller service: %w", err)
		}
		log.Info("Storage controller service updated", "name", cluster.Name)
		return nil
	}

	return nil
}

func (r *ClusterReconciler) reconcileStorageBrokerDeployment(ctx context.Context, cluster *neonv1alpha1.Cluster) error {
	log := logf.FromContext(ctx)

	intendedDeployment := storagebroker.Deployment(cluster)

	var currentDeployment appsv1.Deployment
	getErr := r.Get(ctx, types.NamespacedName{Name: intendedDeployment.Name, Namespace: cluster.Namespace}, &currentDeployment)
	if getErr != nil && !apierrors.IsNotFound(getErr) {
		return fmt.Errorf("failed to get storage broker deployment: %w", getErr)
	}

	err := ctrl.SetControllerReference(cluster, intendedDeployment, r.Scheme)
	if err != nil {
		return fmt.Errorf("failed to set controller reference for storage broker deployment: %w", err)
	}

	// If deployment does not exist, create it
	if apierrors.IsNotFound(getErr) {
		if err := r.Create(ctx, intendedDeployment, &client.CreateOptions{
			FieldManager: utils.FieldManager,
		}); err != nil {
			return fmt.Errorf("failed to create storage broker deployment: %w", err)
		}
		log.Info("Storage broker deployment created", "name", cluster.Name)
		return nil
	}

	// If deployment exists, check if it needs to be updated
	if !equality.Semantic.DeepDerivative(intendedDeployment.Spec, currentDeployment.Spec) {
		// At this point, the deployment exists and needs to be updated
		if err := r.Patch(ctx, intendedDeployment, client.Apply, &client.PatchOptions{
			Force:        ptr.To(true),
			FieldManager: utils.FieldManager,
		}); err != nil {
			return fmt.Errorf("failed to update storage broker deployment: %w", err)
		}
		log.Info("Storage broker deployment updated", "name", cluster.Name)
		return nil
	}

	return nil
}

func (r *ClusterReconciler) reconcileStorageBrokerService(ctx context.Context, cluster *neonv1alpha1.Cluster) error {
	log := logf.FromContext(ctx)

	intendedService := storagebroker.Service(cluster)

	var currentService corev1.Service
	getErr := r.Get(ctx, types.NamespacedName{Name: intendedService.Name, Namespace: cluster.Namespace}, &currentService)
	if getErr != nil && !apierrors.IsNotFound(getErr) {
		return fmt.Errorf("failed to get storage controller service: %w", getErr)
	}

	err := ctrl.SetControllerReference(cluster, intendedService, r.Scheme)
	if err != nil {
		return fmt.Errorf("failed to set controller reference for storage controller service: %w", err)
	}

	// If service does not exist, create it
	if apierrors.IsNotFound(getErr) {
		if err := r.Create(ctx, intendedService, &client.CreateOptions{
			FieldManager: utils.FieldManager,
		}); err != nil {
			return fmt.Errorf("failed to create storage controller service: %w", err)
		}
		log.Info("Storage controller service created", "name", cluster.Name)
		return nil
	}

	// Use DeepDerivative with correct order: intended is subset of current
	if !equality.Semantic.DeepDerivative(intendedService.Spec, currentService.Spec) {
		// At this point, the service exists and needs to be updated
		if err := r.Patch(ctx, intendedService, client.Apply, &client.PatchOptions{
			Force:        ptr.To(true),
			FieldManager: utils.FieldManager,
		}); err != nil {
			return fmt.Errorf("failed to update storage controller service: %w", err)
		}
		log.Info("Storage controller service updated", "name", cluster.Name)
		return nil
	}

	return nil
}
