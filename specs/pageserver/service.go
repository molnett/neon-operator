package pageserver

import (
	"fmt"

	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"oltp.molnett.org/neon-operator/api/v1alpha1"
)

func Service(pageserver *v1alpha1.Pageserver) *corev1.Service {
	serviceName := fmt.Sprintf("%s-pageserver-%d", pageserver.Spec.Cluster, pageserver.Spec.ID)

	return &corev1.Service{
		TypeMeta: metav1.TypeMeta{
			APIVersion: "v1",
			Kind:       "Service",
		},
		ObjectMeta: metav1.ObjectMeta{
			Name:      serviceName,
			Namespace: pageserver.Namespace,
			Labels: map[string]string{
				"molnett.org/cluster":    pageserver.Spec.Cluster,
				"molnett.org/component":  "pageserver",
				"molnett.org/pageserver": pageserver.Name,
			},
		},
		Spec: corev1.ServiceSpec{
			Selector: map[string]string{
				"molnett.org/pageserver": pageserver.Name,
			},
			Ports: []corev1.ServicePort{
				{
					Name:     "pg",
					Port:     6400,
					Protocol: corev1.ProtocolTCP,
				},
				{
					Name:     "http",
					Port:     9898,
					Protocol: corev1.ProtocolTCP,
				},
			},
		},
	}
}
