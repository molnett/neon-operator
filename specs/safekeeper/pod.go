package safekeeper

import (
	"fmt"

	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"oltp.molnett.org/neon-operator/api/v1alpha1"
)

func Pod(safekeeper *v1alpha1.Safekeeper, image string) *corev1.Pod {
	podName := fmt.Sprintf("%s-safekeeper-%d", safekeeper.Spec.Cluster, safekeeper.Spec.ID)
	safekeeperCommand := fmt.Sprintf(
		"/usr/local/bin/safekeeper --id=%d --broker-endpoint=http://%s-storage-broker:50051 --listen-pg=0.0.0.0:5454 --listen-http=0.0.0.0:7676 --advertise-pg=%s:5454 --datadir /data",
		safekeeper.Spec.ID,
		safekeeper.Spec.Cluster,
		podName,
	)

	return &corev1.Pod{
		TypeMeta: metav1.TypeMeta{
			APIVersion: "v1",
			Kind:       "Pod",
		},
		ObjectMeta: metav1.ObjectMeta{
			Name:      podName,
			Namespace: safekeeper.Namespace,
			Labels: map[string]string{
				"molnett.org/cluster":    safekeeper.Spec.Cluster,
				"molnett.org/component":  "safekeeper",
				"molnett.org/safekeeper": safekeeper.Name,
			},
		},
		Spec: corev1.PodSpec{
			SecurityContext: &corev1.PodSecurityContext{
				RunAsUser:  &[]int64{1000}[0],
				RunAsGroup: &[]int64{1000}[0],
				FSGroup:    &[]int64{1000}[0],
			},
			Containers: []corev1.Container{
				{
					Name:    "safekeeper",
					Image:   image,
					Command: []string{"/bin/bash"},
					Args:    []string{"-c", safekeeperCommand},
					Ports: []corev1.ContainerPort{
						{
							ContainerPort: 5454,
						},
						{
							ContainerPort: 7676,
						},
					},
					Env: []corev1.EnvVar{
						{
							Name:  "DEFAULT_PG_VERSION",
							Value: "15",
						},
						{
							Name: "POD_NAME",
							ValueFrom: &corev1.EnvVarSource{
								FieldRef: &corev1.ObjectFieldSelector{
									FieldPath: "metadata.name",
								},
							},
						},
					},
					VolumeMounts: []corev1.VolumeMount{
						{
							Name:      "safekeeper-storage",
							MountPath: "/data",
						},
					},
				},
			},
			Volumes: []corev1.Volume{
				{
					Name: "safekeeper-storage",
					VolumeSource: corev1.VolumeSource{
						PersistentVolumeClaim: &corev1.PersistentVolumeClaimVolumeSource{
							ClaimName: fmt.Sprintf("safekeeper-%s-storage", safekeeper.Name),
						},
					},
				},
			},
		},
	}
}
