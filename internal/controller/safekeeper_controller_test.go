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

	. "github.com/onsi/ginkgo/v2"
	. "github.com/onsi/gomega"
	corev1 "k8s.io/api/core/v1"
	"k8s.io/apimachinery/pkg/api/errors"
	"k8s.io/apimachinery/pkg/types"
	"sigs.k8s.io/controller-runtime/pkg/reconcile"

	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"

	neonv1alpha1 "oltp.molnett.org/neon-operator/api/v1alpha1"
)

var _ = Describe("Safekeeper Controller", func() {
	Context("When reconciling a resource", func() {
		const resourceName = "test-resource"
		const clusterName = "test-cluster"

		ctx := context.Background()

		typeNamespacedName := types.NamespacedName{
			Name:      resourceName,
			Namespace: "default", // TODO(user):Modify as needed
		}
		clusterNamespacedName := types.NamespacedName{
			Name:      clusterName,
			Namespace: "default",
		}
		cluster := &neonv1alpha1.Cluster{}
		safekeeper := &neonv1alpha1.Safekeeper{}

		BeforeEach(func() {
			By("Creating the parent cluster resource")
			cluster_err := k8sClient.Get(ctx, clusterNamespacedName, cluster)
			if cluster_err != nil && errors.IsNotFound(cluster_err) {
				clusterResource := &neonv1alpha1.Cluster{
					ObjectMeta: metav1.ObjectMeta{
						Name:      clusterName,
						Namespace: "default",
					},
					Spec: neonv1alpha1.ClusterSpec{
						NumSafekeepers:   3,
						DefaultPGVersion: 16,
						NeonImage:        "neondatabase/neon:8463",
						BucketCredentialsSecret: &corev1.SecretReference{
							Name:      "test-bucket-secret",
							Namespace: "default",
						},
						StorageControllerDatabaseSecret: &corev1.SecretKeySelector{
							LocalObjectReference: corev1.LocalObjectReference{
								Name: "test-db-secret",
							},
							Key: "uri",
						},
					},
				}
				Expect(k8sClient.Create(ctx, clusterResource)).To(Succeed())
			}

			By("Creating the custom resource for the Kind Safekeeper")
			safekeeper_err := k8sClient.Get(ctx, typeNamespacedName, safekeeper)
			if safekeeper_err != nil && errors.IsNotFound(safekeeper_err) {
				resource := &neonv1alpha1.Safekeeper{
					ObjectMeta: metav1.ObjectMeta{
						Name:      resourceName,
						Namespace: "default",
					},
					Spec: neonv1alpha1.SafekeeperSpec{
						ID:      1,
						Cluster: clusterName,
						StorageConfig: neonv1alpha1.StorageConfig{
							Size: "10Gi",
						},
					},
				}
				Expect(k8sClient.Create(ctx, resource)).To(Succeed())
			}
		})

		AfterEach(func() {
			// TODO(user): Cleanup logic after each test, like removing the resource instance.
			resource := &neonv1alpha1.Safekeeper{}
			err := k8sClient.Get(ctx, typeNamespacedName, resource)
			Expect(err).NotTo(HaveOccurred())

			By("Cleanup the specific resource instance Safekeeper")
			Expect(k8sClient.Delete(ctx, resource)).To(Succeed())

			cluster := &neonv1alpha1.Cluster{}
			err = k8sClient.Get(ctx, clusterNamespacedName, cluster)
			Expect(err).NotTo(HaveOccurred())

			By("Cleanup the specific resource instance Cluster")
			Expect(k8sClient.Delete(ctx, cluster)).To(Succeed())
		})
		It("Should successfully reconcile the resources", func() {
			By("Reconciling the created resource")
			controllerReconciler := &SafekeeperReconciler{
				Client: k8sClient,
				Scheme: k8sClient.Scheme(),
			}

			_, err := controllerReconciler.Reconcile(ctx, reconcile.Request{
				NamespacedName: typeNamespacedName,
			})
			Expect(err).NotTo(HaveOccurred())
			// TODO(user): Add more specific assertions depending on your controller's reconciliation logic.
			// Example: If you expect a certain status condition after reconciliation, verify it here.
		})
	})
})
