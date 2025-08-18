package storagebroker

import (
	"fmt"

	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/apimachinery/pkg/util/intstr"
	"oltp.molnett.org/neon-operator/api/v1alpha1"
)

func Service(cluster *v1alpha1.Cluster) *corev1.Service {
	storageBrokerName := fmt.Sprintf("%s-storage-broker", cluster.Name)

	return &corev1.Service{
		ObjectMeta: metav1.ObjectMeta{
			Name:      fmt.Sprintf("%s-storage-broker", cluster.Name),
			Namespace: cluster.Namespace,
			Labels: map[string]string{
				"app.kubernetes.io/name": storageBrokerName,
			},
		},
		Spec: corev1.ServiceSpec{
			Selector: map[string]string{
				"app.kubernetes.io/name": storageBrokerName,
			},
			Ports: []corev1.ServicePort{
				{
					Name:       "http",
					Port:       50051,
					TargetPort: intstr.FromInt(50051),
				},
			},
		},
	}
}
