package storagecontroller

import (
	"fmt"

	appsv1 "k8s.io/api/apps/v1"
	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/utils/ptr"
	"oltp.molnett.org/neon-operator/api/v1alpha1"
)

func Deployment(cluster *v1alpha1.Cluster) *appsv1.Deployment {
	storageControllerName := fmt.Sprintf("%s-storage-controller", cluster.Name)

	return &appsv1.Deployment{
		TypeMeta: metav1.TypeMeta{
			APIVersion: "apps/v1",
			Kind:       "Deployment",
		},
		ObjectMeta: metav1.ObjectMeta{
			Name:      storageControllerName,
			Namespace: cluster.Namespace,
		},
		Spec: appsv1.DeploymentSpec{
			Replicas: ptr.To(int32(1)),
			Strategy: appsv1.DeploymentStrategy{
				Type: appsv1.RollingUpdateDeploymentStrategyType,
			},
			Selector: &metav1.LabelSelector{
				MatchLabels: map[string]string{
					"app.kubernetes.io/name": storageControllerName,
				},
			},
			Template: corev1.PodTemplateSpec{
				ObjectMeta: metav1.ObjectMeta{
					Labels: map[string]string{
						"app.kubernetes.io/name": storageControllerName,
					},
				},
				Spec: corev1.PodSpec{
					Containers: []corev1.Container{
						{
							Name:            "storage-controller",
							Image:           cluster.Spec.NeonImage,
							ImagePullPolicy: corev1.PullIfNotPresent,
							Command: []string{
								"storage_controller",
							},
							Args: []string{
								"--dev",
								"-l",
								"0.0.0.0:8080",
								"--control-plane-url",
								"http://neon-operator:8080",
								"--initial-split-shards",
								"0",
							},
							Env: []corev1.EnvVar{
								{
									Name: "DATABASE_URL",
									ValueFrom: &corev1.EnvVarSource{
										SecretKeyRef: &corev1.SecretKeySelector{
											LocalObjectReference: corev1.LocalObjectReference{
												Name: cluster.Spec.StorageControllerDatabaseSecret.Name,
											},
											Key: cluster.Spec.StorageControllerDatabaseSecret.Key,
										},
									},
								},
							},
							Ports: []corev1.ContainerPort{
								{
									Name:          "http",
									ContainerPort: 8080,
								},
							},
						},
					},
				},
			},
		},
	}
}
