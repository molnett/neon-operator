package compute

import (
	"fmt"

	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/apimachinery/pkg/util/intstr"

	neonv1alpha1 "oltp.molnett.org/neon-operator/api/v1alpha1"
)

func AdminService(branch *neonv1alpha1.Branch, project *neonv1alpha1.Project) *corev1.Service {
	serviceName := fmt.Sprintf("%s-admin", branch.Name)
	labels := map[string]string{
		"molnett.org/cluster":   project.Spec.ClusterName,
		"molnett.org/component": "compute-admin",
		"molnett.org/branch":    branch.Name,
		"neon.timeline_id":      branch.Spec.TimelineID,
		"neon.tenant_id":        project.Spec.TenantID,
	}

	return &corev1.Service{
		TypeMeta: metav1.TypeMeta{
			APIVersion: "v1",
			Kind:       "Service",
		},
		ObjectMeta: metav1.ObjectMeta{
			Name:      serviceName,
			Namespace: branch.Namespace,
			Labels:    labels,
		},
		Spec: corev1.ServiceSpec{
			Selector: map[string]string{
				"app": fmt.Sprintf("%s-compute-node", branch.Name),
			},
			Ports: []corev1.ServicePort{
				{
					Name:       "admin",
					Port:       3080,
					Protocol:   corev1.ProtocolTCP,
					TargetPort: intstr.FromInt(3080),
				},
			},
		},
	}
}

func PostgresService(branch *neonv1alpha1.Branch, project *neonv1alpha1.Project) *corev1.Service {
	serviceName := fmt.Sprintf("%s-postgres", branch.Name)
	labels := map[string]string{
		"molnett.org/cluster":   project.Spec.ClusterName,
		"molnett.org/component": "compute-postgres",
		"molnett.org/branch":    branch.Name,
		"neon.timeline_id":      branch.Spec.TimelineID,
		"neon.tenant_id":        project.Spec.TenantID,
	}

	return &corev1.Service{
		TypeMeta: metav1.TypeMeta{
			APIVersion: "v1",
			Kind:       "Service",
		},
		ObjectMeta: metav1.ObjectMeta{
			Name:      serviceName,
			Namespace: branch.Namespace,
			Labels:    labels,
		},
		Spec: corev1.ServiceSpec{
			Selector: map[string]string{
				"app": fmt.Sprintf("%s-compute-node", branch.Name),
			},
			Ports: []corev1.ServicePort{
				{
					Name:       "postgres",
					Port:       55433,
					Protocol:   corev1.ProtocolTCP,
					TargetPort: intstr.FromInt(55433),
				},
			},
		},
	}
}
