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
	"context"
	"errors"
	"fmt"

	appsv1 "k8s.io/api/apps/v1"
	corev1 "k8s.io/api/core/v1"
	apierrors "k8s.io/apimachinery/pkg/api/errors"
	"k8s.io/apimachinery/pkg/runtime"
	ctrl "sigs.k8s.io/controller-runtime"
	"sigs.k8s.io/controller-runtime/pkg/client"
	logf "sigs.k8s.io/controller-runtime/pkg/log"

	neonv1alpha1 "oltp.molnett.org/neon-operator/api/v1alpha1"
	"oltp.molnett.org/neon-operator/utils"
)

// This is not a proper error. It indicated we should return a empty requeue after an object has been changed.
var ErrRequeueAfterChange = errors.New("requeue after change")

// ClusterReconciler reconciles a Cluster object
type ClusterReconciler struct {
	client.Client
	Scheme *runtime.Scheme
}

// +kubebuilder:rbac:groups=neon.oltp.molnett.org,resources=clusters,verbs=get;list;watch;create;update;patch;delete
// +kubebuilder:rbac:groups=neon.oltp.molnett.org,resources=clusters/status,verbs=get;update;patch
// +kubebuilder:rbac:groups=neon.oltp.molnett.org,resources=clusters/finalizers,verbs=update
// +kubebuilder:rbac:groups=neon.oltp.molnett.org,resources=projects,verbs=get;list;watch;create;update;patch;delete
// +kubebuilder:rbac:groups=neon.oltp.molnett.org,resources=projects/status,verbs=get;update;patch
// +kubebuilder:rbac:groups=neon.oltp.molnett.org,resources=projects/finalizers,verbs=update
// +kubebuilder:rbac:groups=neon.oltp.molnett.org,resources=branches,verbs=get;list;watch;create;update;patch;delete
// +kubebuilder:rbac:groups=neon.oltp.molnett.org,resources=branches/status,verbs=get;update;patch
// +kubebuilder:rbac:groups=neon.oltp.molnett.org,resources=branches/finalizers,verbs=update
// +kubebuilder:rbac:groups=apps,resources=deployments,verbs=get;list;watch;create;update;patch;delete
// +kubebuilder:rbac:groups="",resources=services,verbs=get;list;watch;create;update;patch;delete
// +kubebuilder:rbac:groups="",resources=configmaps,verbs=get;list;watch;create;update;patch;delete
// +kubebuilder:rbac:groups="",resources=secrets,verbs=get;list;watch;create;update;patch;delete

func (r *ClusterReconciler) Reconcile(ctx context.Context, req ctrl.Request) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	log.Info("Reconcile loop start", "request", req)
	defer func() {
		log.Info("Reconcile loop end", "request", req)
	}()

	cluster, err := r.getCluster(ctx, req)
	if err != nil {
		return ctrl.Result{}, err
	}

	ctx = context.WithValue(ctx, utils.ClusterNameKey, cluster.Name)

	result, err := r.reconcile(ctx, cluster)
	if errors.Is(err, ErrRequeueAfterChange) {
		return result, nil
	} else if err != nil {
		log.Error(err, "Reconcile failed")
		return ctrl.Result{}, err
	}

	return result, nil
}

//nolint:unparam
func (r *ClusterReconciler) reconcile(ctx context.Context, cluster *neonv1alpha1.Cluster) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	if cluster.Status.Phase == "" {
		if err := utils.SetPhases(ctx, r.Client, cluster, utils.SetClusterCreatingStatus); err != nil {
			return ctrl.Result{}, fmt.Errorf("error setting default Status: %w", err)
		}
		log.Info("Cluster phase set to creating")
	}

	err := r.createClusterResources(ctx, cluster)
	if err != nil {
		log.Error(err, "error while creating cluster resources")
		if setErr := utils.SetPhases(ctx, r.Client, cluster, utils.SetClusterCannotCreateResourcesStatus); setErr != nil {
			log.Error(setErr, "failed to set cluster status")
		}
		return ctrl.Result{}, fmt.Errorf("not able to create cluster resources: %w", err)
	}

	// Set cluster to ready status after successful resource creation
	if err := utils.SetPhases(ctx, r.Client, cluster, utils.SetClusterReadyStatus); err != nil {
		log.Error(err, "failed to set cluster ready status")
		return ctrl.Result{}, fmt.Errorf("failed to update cluster status to ready: %w", err)
	}
	log.Info("Cluster status set to ready")

	return ctrl.Result{}, nil
}

func (r *ClusterReconciler) getCluster(ctx context.Context, req ctrl.Request) (*neonv1alpha1.Cluster, error) {
	log := logf.FromContext(ctx)
	cluster := &neonv1alpha1.Cluster{}
	if err := r.Get(ctx, req.NamespacedName, cluster); err != nil {
		if apierrors.IsNotFound(err) {
			log.Info("Cluster has been deleted")
			return nil, nil
		}

		return nil, fmt.Errorf("cannot get the resource: %w", err)
	}
	return cluster, nil
}

// SetupWithManager sets up the controller with the Manager.
func (r *ClusterReconciler) SetupWithManager(mgr ctrl.Manager) error {
	return ctrl.NewControllerManagedBy(mgr).
		For(&neonv1alpha1.Cluster{}).
		Owns(&appsv1.Deployment{}).
		Owns(&corev1.Service{}).
		Named("cluster").
		Complete(r)
}
