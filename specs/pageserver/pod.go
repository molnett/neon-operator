package pageserver

import (
	"fmt"

	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"oltp.molnett.org/neon-operator/api/v1alpha1"
)

func Pod(pageserver *v1alpha1.Pageserver, image string) *corev1.Pod {
	podName := fmt.Sprintf("%s-pageserver-%d", pageserver.Spec.Cluster, pageserver.Spec.ID)

	return &corev1.Pod{
		TypeMeta: metav1.TypeMeta{
			APIVersion: "v1",
			Kind:       "Pod",
		},
		ObjectMeta: metav1.ObjectMeta{
			Name:      podName,
			Namespace: pageserver.Namespace,
			Labels: map[string]string{
				"molnett.org/cluster":    pageserver.Spec.Cluster,
				"molnett.org/component":  "pageserver",
				"molnett.org/pageserver": pageserver.Name,
			},
			Finalizers: []string{
				"pageserver.neon.io/finalizer",
			},
		},
		Spec: corev1.PodSpec{
			SecurityContext: &corev1.PodSecurityContext{
				RunAsUser:  &[]int64{1000}[0],
				RunAsGroup: &[]int64{1000}[0],
				FSGroup:    &[]int64{1000}[0],
			},
			InitContainers: []corev1.Container{
				{
					Name:    "setup-config",
					Image:   "busybox:latest",
					Command: []string{"/bin/sh", "-c"},
					Args: []string{
						fmt.Sprintf(`
# Use the pageserver ID directly
echo "id=%d" > /config/identity.toml

# Create metadata.json with proper host information using service DNS
echo "{\"host\":\"%s-pageserver-%d.%s\"," \
     "\"http_host\":\"%s-pageserver-%d.%s\"," \
     "\"http_port\":9898,\"port\":6400," \
     "\"availability_zone_id\":\"se-ume\"}" > /config/metadata.json

# Copy pageserver.toml from configmap
cp /configmap/pageserver.toml /config/pageserver.toml
						`,
							pageserver.Spec.ID,
							pageserver.Spec.Cluster, pageserver.Spec.ID, pageserver.Namespace,
							pageserver.Spec.Cluster, pageserver.Spec.ID, pageserver.Namespace),
					},
					VolumeMounts: []corev1.VolumeMount{
						{
							Name:      "pageserver-config",
							MountPath: "/configmap",
						},
						{
							Name:      "config",
							MountPath: "/config",
						},
					},
				},
			},
			Containers: []corev1.Container{
				{
					Name:            "pageserver",
					Image:           image,
					ImagePullPolicy: corev1.PullAlways,
					Command:         []string{"/usr/local/bin/pageserver"},
					Ports: []corev1.ContainerPort{
						{
							ContainerPort: 6400,
						},
						{
							ContainerPort: 9898,
						},
					},
					Env: []corev1.EnvVar{
						{
							Name:  "RUST_LOG",
							Value: "debug",
						},
						{
							Name:  "DEFAULT_PG_VERSION",
							Value: "16",
						},
						{
							Name: "AWS_ACCESS_KEY_ID",
							ValueFrom: &corev1.EnvVarSource{
								SecretKeyRef: &corev1.SecretKeySelector{
									LocalObjectReference: corev1.LocalObjectReference{
										Name: pageserver.Spec.BucketCredentialsSecret.Name,
									},
									Key: "AWS_ACCESS_KEY_ID",
								},
							},
						},
						{
							Name: "AWS_SECRET_ACCESS_KEY",
							ValueFrom: &corev1.EnvVarSource{
								SecretKeyRef: &corev1.SecretKeySelector{
									LocalObjectReference: corev1.LocalObjectReference{
										Name: pageserver.Spec.BucketCredentialsSecret.Name,
									},
									Key: "AWS_SECRET_ACCESS_KEY",
								},
							},
						},
						{
							Name: "AWS_REGION",
							ValueFrom: &corev1.EnvVarSource{
								SecretKeyRef: &corev1.SecretKeySelector{
									LocalObjectReference: corev1.LocalObjectReference{
										Name: pageserver.Spec.BucketCredentialsSecret.Name,
									},
									Key: "AWS_REGION",
								},
							},
						},
						{
							Name: "BUCKET_NAME",
							ValueFrom: &corev1.EnvVarSource{
								SecretKeyRef: &corev1.SecretKeySelector{
									LocalObjectReference: corev1.LocalObjectReference{
										Name: pageserver.Spec.BucketCredentialsSecret.Name,
									},
									Key: "BUCKET_NAME",
								},
							},
						},
						{
							Name: "AWS_ENDPOINT_URL",
							ValueFrom: &corev1.EnvVarSource{
								SecretKeyRef: &corev1.SecretKeySelector{
									LocalObjectReference: corev1.LocalObjectReference{
										Name: pageserver.Spec.BucketCredentialsSecret.Name,
									},
									Key: "AWS_ENDPOINT_URL",
								},
							},
						},
					},
					VolumeMounts: []corev1.VolumeMount{
						{
							Name:      "pageserver-storage",
							MountPath: "/data/.neon/tenants",
						},
						{
							Name:      "config",
							MountPath: "/data/.neon",
						},
					},
				},
			},
			Volumes: []corev1.Volume{
				{
					Name: "pageserver-storage",
					VolumeSource: corev1.VolumeSource{
						PersistentVolumeClaim: &corev1.PersistentVolumeClaimVolumeSource{
							ClaimName: podName,
						},
					},
				},
				{
					Name: "pageserver-config",
					VolumeSource: corev1.VolumeSource{
						ConfigMap: &corev1.ConfigMapVolumeSource{
							LocalObjectReference: corev1.LocalObjectReference{
								Name: podName,
							},
						},
					},
				},
				{
					Name: "config",
					VolumeSource: corev1.VolumeSource{
						EmptyDir: &corev1.EmptyDirVolumeSource{},
					},
				},
			},
		},
	}
}
