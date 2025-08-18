package storagebroker

import (
	"fmt"

	appsv1 "k8s.io/api/apps/v1"
	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/utils/ptr"
	"oltp.molnett.org/neon-operator/api/v1alpha1"
)

func Deployment(cluster *v1alpha1.Cluster) *appsv1.Deployment {
	storageBrokerName := fmt.Sprintf("%s-storage-broker", cluster.Name)

	return &appsv1.Deployment{
		TypeMeta: metav1.TypeMeta{
			APIVersion: "apps/v1",
			Kind:       "Deployment",
		},
		ObjectMeta: metav1.ObjectMeta{
			Name:      storageBrokerName,
			Namespace: cluster.Namespace,
		},
		Spec: appsv1.DeploymentSpec{
			Replicas: ptr.To(int32(1)),
			Strategy: appsv1.DeploymentStrategy{
				Type: appsv1.RollingUpdateDeploymentStrategyType,
			},
			Selector: &metav1.LabelSelector{
				MatchLabels: map[string]string{
					"app.kubernetes.io/name": storageBrokerName,
				},
			},
			Template: corev1.PodTemplateSpec{
				ObjectMeta: metav1.ObjectMeta{
					Labels: map[string]string{
						"app.kubernetes.io/name": storageBrokerName,
					},
				},
				Spec: corev1.PodSpec{
					Containers: []corev1.Container{
						{
							Name:            "storage-broker",
							Image:           cluster.Spec.NeonImage,
							ImagePullPolicy: corev1.PullIfNotPresent,
							Command: []string{
								"storage_broker",
							},
							Args: []string{
								"--listen-addr", "0.0.0.0:50051",
							},
							Ports: []corev1.ContainerPort{
								{
									Name:          "http",
									ContainerPort: 50051,
								},
							},
						},
					},
				},
			},
		},
	}
}
