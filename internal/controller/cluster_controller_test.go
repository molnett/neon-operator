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

var _ = Describe("Cluster Controller", func() {
	Context("When reconciling a resource", func() {
		const resourceName = "test-resource"
		const bucketSecretName = "test-bucket-secret"
		const dbSecretName = "test-db-secret"

		ctx := context.Background()

		typeNamespacedName := types.NamespacedName{
			Name:      resourceName,
			Namespace: "default", // TODO(user):Modify as needed
		}
		cluster := &neonv1alpha1.Cluster{}

		BeforeEach(func() {
			By("Creating required secrets")

			bucketSecret := &corev1.Secret{
				ObjectMeta: metav1.ObjectMeta{
					Name:      bucketSecretName,
					Namespace: "default",
				},
				Data: map[string][]byte{
					"access-key-id":     []byte("test-access-key"),
					"secret-access-key": []byte("test-secret-key"),
					"region":            []byte("us-east-1"),
				},
			}
			err := k8sClient.Get(ctx, types.NamespacedName{Name: bucketSecretName, Namespace: "default"}, &corev1.Secret{})
			if err != nil && errors.IsNotFound(err) {
				Expect(k8sClient.Create(ctx, bucketSecret)).To(Succeed())
			}

			dbSecret := &corev1.Secret{
				ObjectMeta: metav1.ObjectMeta{
					Name:      dbSecretName,
					Namespace: "default",
				},
				Data: map[string][]byte{
					"uri": []byte("postgresql://user:pass@localhost:5432/db"),
				},
			}
			err = k8sClient.Get(ctx, types.NamespacedName{Name: dbSecretName, Namespace: "default"}, &corev1.Secret{})
			if err != nil && errors.IsNotFound(err) {
				Expect(k8sClient.Create(ctx, dbSecret)).To(Succeed())
			}

			By("Creating the custom resource for the Kind Cluster")
			err = k8sClient.Get(ctx, typeNamespacedName, cluster)
			if err != nil && errors.IsNotFound(err) {
				resource := &neonv1alpha1.Cluster{
					ObjectMeta: metav1.ObjectMeta{
						Name:      resourceName,
						Namespace: "default",
					},
					Spec: neonv1alpha1.ClusterSpec{
						NumSafekeepers:   3,
						DefaultPGVersion: 16,
						NeonImage:        "neondatabase/neon:8463",
						BucketCredentialsSecret: &corev1.SecretReference{
							Name:      bucketSecretName,
							Namespace: "default",
						},
						StorageControllerDatabaseSecret: &corev1.SecretKeySelector{
							LocalObjectReference: corev1.LocalObjectReference{
								Name: dbSecretName,
							},
							Key: "uri",
						},
					},
				}
				Expect(k8sClient.Create(ctx, resource)).To(Succeed())
			}
		})

		AfterEach(func() {
			// TODO(user): Cleanup logic after each test, like removing the resource instance.
			resource := &neonv1alpha1.Cluster{}
			err := k8sClient.Get(ctx, typeNamespacedName, resource)
			Expect(err).NotTo(HaveOccurred())

			By("Cleanup the specific resource instance Cluster")
			Expect(k8sClient.Delete(ctx, resource)).To(Succeed())

			By("Cleanup the test secrets")
			bucketSecret := &corev1.Secret{}
			err = k8sClient.Get(ctx, types.NamespacedName{Name: bucketSecretName, Namespace: "default"}, bucketSecret)
			Expect(err).NotTo(HaveOccurred())
			Expect(k8sClient.Delete(ctx, bucketSecret)).To(Succeed())

			dbSecret := &corev1.Secret{}
			err = k8sClient.Get(ctx, types.NamespacedName{Name: dbSecretName, Namespace: "default"}, dbSecret)
			Expect(err).NotTo(HaveOccurred())
			Expect(k8sClient.Delete(ctx, dbSecret)).To(Succeed())
		})
		It("Should successfully reconcile the resource", func() {
			By("Reconciling the created resource")
			controllerReconciler := &ClusterReconciler{
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
