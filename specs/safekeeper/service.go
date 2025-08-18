package safekeeper

import (
	"fmt"

	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"oltp.molnett.org/neon-operator/api/v1alpha1"
)

func Service(safekeeper *v1alpha1.Safekeeper) *corev1.Service {
	serviceName := fmt.Sprintf("safekeeper-%s", safekeeper.Name)

	return &corev1.Service{
		TypeMeta: metav1.TypeMeta{
			APIVersion: "v1",
			Kind:       "Service",
		},
		ObjectMeta: metav1.ObjectMeta{
			Name:      serviceName,
			Namespace: safekeeper.Namespace,
		},
		Spec: corev1.ServiceSpec{
			Selector: map[string]string{
				"molnett.org/safekeeper": safekeeper.Name,
			},
			Ports: []corev1.ServicePort{
				{
					Name:     "pg",
					Port:     5454,
					Protocol: corev1.ProtocolTCP,
				},
				{
					Name:     "http",
					Port:     7676,
					Protocol: corev1.ProtocolTCP,
				},
			},
		},
	}
}
