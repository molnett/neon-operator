package compute

import (
	"fmt"

	appsv1 "k8s.io/api/apps/v1"
	corev1 "k8s.io/api/core/v1"
	"k8s.io/apimachinery/pkg/api/resource"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/utils/ptr"

	neonv1alpha1 "oltp.molnett.org/neon-operator/api/v1alpha1"
)

func Deployment(branch *neonv1alpha1.Branch, project *neonv1alpha1.Project) *appsv1.Deployment {
	deploymentName := fmt.Sprintf("%s-compute-node", branch.Name)

	labels := map[string]string{
		"app":                   deploymentName,
		"molnett.org/cluster":   project.Spec.ClusterName,
		"molnett.org/component": "compute",
		"molnett.org/branch":    branch.Name,
		"neon.tenant_id":        project.Spec.TenantID,
		"neon.timeline_id":      branch.Spec.TimelineID,
	}

	annotations := map[string]string{
		"neon.compute_id":   branch.Name,
		"neon.cluster_name": project.Spec.ClusterName,
	}

	return &appsv1.Deployment{
		TypeMeta: metav1.TypeMeta{
			APIVersion: "apps/v1",
			Kind:       "Deployment",
		},
		ObjectMeta: metav1.ObjectMeta{
			Name:        deploymentName,
			Namespace:   branch.Namespace,
			Labels:      labels,
			Annotations: annotations,
		},
		Spec: appsv1.DeploymentSpec{
			Replicas: ptr.To(int32(1)),
			Selector: &metav1.LabelSelector{
				MatchLabels: labels,
			},
			Template: corev1.PodTemplateSpec{
				ObjectMeta: metav1.ObjectMeta{
					Labels: labels,
				},
				Spec: corev1.PodSpec{
					SecurityContext: &corev1.PodSecurityContext{
						RunAsUser: ptr.To(int64(1000)),
						FSGroup:   ptr.To(int64(1000)),
					},
					Containers: []corev1.Container{
						{
							Name:  "compute-node",
							Image: fmt.Sprintf("neondatabase/compute-node-v%d", branch.Spec.PGVersion),
							Command: []string{
								"bash",
								"-c",
								fmt.Sprintf(
									"echo \"$INITIAL_SPEC_JSON\" > /var/spec.json && "+
										"/usr/local/bin/compute_ctl --pgdata /.neon/data/pgdata "+
										"--connstr=postgresql://cloud_admin:@0.0.0.0:55433/postgres "+
										"--compute-id %s -p http://neon-controlplane.neon:8081 "+
										"--pgbin /usr/local/bin/postgres",
									branch.Name,
								),
							},
							Ports: []corev1.ContainerPort{
								{
									ContainerPort: 55433,
									Protocol:      corev1.ProtocolTCP,
								},
							},
							VolumeMounts: []corev1.VolumeMount{
								{
									Name:      "spec-volume",
									MountPath: "/var",
								},
								{
									Name:      "pgdata",
									MountPath: "/.neon/data",
								},
							},
							Env: []corev1.EnvVar{
								{
									Name:  "OTEL_SDK_DISABLED",
									Value: "true",
								},
								{
									Name: "INITIAL_SPEC_JSON",
									ValueFrom: &corev1.EnvVarSource{
										ConfigMapKeyRef: &corev1.ConfigMapKeySelector{
											LocalObjectReference: corev1.LocalObjectReference{
												Name: fmt.Sprintf("%s-compute-spec", branch.Name),
											},
											Key: "spec.json",
										},
									},
								},
							},
						},
					},
					Volumes: []corev1.Volume{
						{
							Name: "spec-volume",
							VolumeSource: corev1.VolumeSource{
								EmptyDir: &corev1.EmptyDirVolumeSource{},
							},
						},
						{
							Name: "pgdata",
							VolumeSource: corev1.VolumeSource{
								EmptyDir: &corev1.EmptyDirVolumeSource{
									SizeLimit: &[]resource.Quantity{resource.MustParse("500Mi")}[0],
								},
							},
						},
					},
				},
			},
		},
	}
}
