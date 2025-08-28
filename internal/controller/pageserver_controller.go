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

// PageserverReconciler reconciles a Pageserver object
type PageserverReconciler struct {
	client.Client
	Scheme *runtime.Scheme
}

// +kubebuilder:rbac:groups=neon.oltp.molnett.org,resources=pageservers,verbs=get;list;watch;create;update;patch;delete
// +kubebuilder:rbac:groups=neon.oltp.molnett.org,resources=pageservers/status,verbs=get;update;patch
// +kubebuilder:rbac:groups=neon.oltp.molnett.org,resources=pageservers/finalizers,verbs=update
// +kubebuilder:rbac:groups="",resources=persistentvolumeclaims,verbs=get;list;watch;create;update;patch;delete
// +kubebuilder:rbac:groups="",resources=pods,verbs=get;list;watch;create;update;patch;delete
// +kubebuilder:rbac:groups="",resources=services,verbs=get;list;watch;create;update;patch;delete
// +kubebuilder:rbac:groups="",resources=configmaps,verbs=get;list;watch;create;update;patch;delete

func (r *PageserverReconciler) Reconcile(ctx context.Context, req ctrl.Request) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	log.Info("Reconcile loop start", "request", req)
	defer func() {
		log.Info("Reconcile loop end", "request", req)
	}()

	pageserver, err := r.getPageserver(ctx, req)
	if err != nil || pageserver == nil {
		return ctrl.Result{}, err
	}

	ctx = context.WithValue(ctx, utils.PageserverNameKey, pageserver.Name)

	result, err := r.reconcile(ctx, pageserver)
	if errors.Is(err, ErrRequeueAfterChange) {
		return result, nil
	} else if err != nil {
		log.Error(err, "Reconcile failed")
		return ctrl.Result{}, err
	}

	return result, nil
}

func (r *PageserverReconciler) getPageserver(ctx context.Context, req ctrl.Request) (*neonv1alpha1.Pageserver, error) {
	log := logf.FromContext(ctx)
	pageserver := &neonv1alpha1.Pageserver{}
	if err := r.Get(ctx, req.NamespacedName, pageserver); err != nil {
		if apierrors.IsNotFound(err) {
			log.Info("Pageserver has been deleted")
			return nil, nil
		}

		return nil, fmt.Errorf("cannot get the resource: %w", err)
	}
	return pageserver, nil
}

//nolint:unparam
func (r *PageserverReconciler) reconcile(ctx context.Context, pageserver *neonv1alpha1.Pageserver) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	if pageserver.Status.Phase == "" {
		if err := utils.SetPhases(ctx, r.Client, pageserver, utils.SetPageserverCreatingStatus); err != nil {
			return ctrl.Result{}, fmt.Errorf("error setting default Status: %w", err)
		}
		log.Info("Pageserver phase set to creating")
	}

	err := r.createPageserverResources(ctx, pageserver)
	if err != nil {
		log.Error(err, "error while creating pageserver resources")
		if setErr := utils.SetPhases(ctx, r.Client, pageserver, utils.SetPageserverCannotCreateResourcesStatus); setErr != nil {
			log.Error(setErr, "failed to set pageserver status")
		}
		return ctrl.Result{}, fmt.Errorf("not able to create pageserver resources: %w", err)
	}

	return ctrl.Result{}, nil
}

// SetupWithManager sets up the controller with the Manager.
func (r *PageserverReconciler) SetupWithManager(mgr ctrl.Manager) error {
	return ctrl.NewControllerManagedBy(mgr).
		For(&neonv1alpha1.Pageserver{}).
		Owns(&corev1.PersistentVolumeClaim{}).
		Owns(&corev1.Pod{}).
		Owns(&corev1.Service{}).
		Owns(&corev1.ConfigMap{}).
		Named("pageserver").
		Complete(r)
}
