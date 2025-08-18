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

	corev1 "k8s.io/api/core/v1"
	apierrors "k8s.io/apimachinery/pkg/api/errors"
	"k8s.io/apimachinery/pkg/runtime"
	ctrl "sigs.k8s.io/controller-runtime"
	"sigs.k8s.io/controller-runtime/pkg/client"
	logf "sigs.k8s.io/controller-runtime/pkg/log"

	neonv1alpha1 "oltp.molnett.org/neon-operator/api/v1alpha1"
	"oltp.molnett.org/neon-operator/utils"
)

// SafekeeperReconciler reconciles a Safekeeper object
type SafekeeperReconciler struct {
	client.Client
	Scheme *runtime.Scheme
}

// +kubebuilder:rbac:groups=neon.oltp.molnett.org,resources=safekeepers,verbs=get;list;watch;create;update;patch;delete
// +kubebuilder:rbac:groups=neon.oltp.molnett.org,resources=safekeepers/status,verbs=get;update;patch
// +kubebuilder:rbac:groups=neon.oltp.molnett.org,resources=safekeepers/finalizers,verbs=update
// +kubebuilder:rbac:groups="",resources=persistentvolumeclaims,verbs=get;list;watch;create;update;patch;delete
// +kubebuilder:rbac:groups="",resources=pods,verbs=get;list;watch;create;update;patch;delete
// +kubebuilder:rbac:groups="",resources=services,verbs=get;list;watch;create;update;patch;delete

func (r *SafekeeperReconciler) Reconcile(ctx context.Context, req ctrl.Request) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	log.Info("Reconcile loop start", "request", req)
	defer func() {
		log.Info("Reconcile loop end", "request", req)
	}()

	safekeeper, err := r.getSafekeeper(ctx, req)
	if err != nil || safekeeper == nil {
		return ctrl.Result{}, err
	}

	ctx = context.WithValue(ctx, utils.SafekeeperNameKey, safekeeper.Name)

	result, err := r.reconcile(ctx, safekeeper)
	if errors.Is(err, ErrRequeueAfterChange) {
		return result, nil
	} else if err != nil {
		log.Error(err, "Reconcile failed")
		return ctrl.Result{}, err
	}

	return result, nil
}

func (r *SafekeeperReconciler) getSafekeeper(ctx context.Context, req ctrl.Request) (*neonv1alpha1.Safekeeper, error) {
	log := logf.FromContext(ctx)
	safekeeper := &neonv1alpha1.Safekeeper{}
	if err := r.Get(ctx, req.NamespacedName, safekeeper); err != nil {
		if apierrors.IsNotFound(err) {
			log.Info("Safekeeper has been deleted")
			return nil, nil
		}

		return nil, fmt.Errorf("cannot get the resource: %w", err)
	}
	return safekeeper, nil
}

func (r *SafekeeperReconciler) reconcile(ctx context.Context, safekeeper *neonv1alpha1.Safekeeper) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	if safekeeper.Status.Phase == "" {
		if err := utils.SetPhases(ctx, r.Client, safekeeper, utils.SetSafekeeperCreatingStatus); err != nil {
			return ctrl.Result{}, fmt.Errorf("error setting default Status: %w", err)
		}
		log.Info("Safekeeper phase set to creating")
	}

	err := r.createSafekeeperResources(ctx, safekeeper)
	if err != nil {
		log.Error(err, "error while creating safekeeper resources")
		utils.SetPhases(ctx, r.Client, safekeeper, utils.SetSafekeeperCannotCreateResourcesStatus)
		return ctrl.Result{}, fmt.Errorf("not able to create safekeeper resources: %w", err)
	}

	return ctrl.Result{}, nil
}

// SetupWithManager sets up the controller with the Manager.
func (r *SafekeeperReconciler) SetupWithManager(mgr ctrl.Manager) error {
	return ctrl.NewControllerManagedBy(mgr).
		For(&neonv1alpha1.Safekeeper{}).
		Owns(&corev1.PersistentVolumeClaim{}).
		Owns(&corev1.Pod{}).
		Owns(&corev1.Service{}).
		Named("safekeeper").
		Complete(r)
}
