package pageserver

import (
	"fmt"

	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"oltp.molnett.org/neon-operator/api/v1alpha1"
)

func ConfigMap(pageserver *v1alpha1.Pageserver, bucketSecret *corev1.Secret) *corev1.ConfigMap {
	configMapName := fmt.Sprintf("%s-pageserver-%d", pageserver.Spec.Cluster, pageserver.Spec.ID)

	// Extract values from secret
	bucketName := string(bucketSecret.Data["BUCKET_NAME"])
	awsRegion := string(bucketSecret.Data["AWS_REGION"])
	awsEndpointURL := string(bucketSecret.Data["AWS_ENDPOINT_URL"])

	pageserverToml := fmt.Sprintf(`
control_plane_api = "http://%s-storage-controller:8080/upcall/v1/"
listen_pg_addr = "0.0.0.0:6400"
listen_http_addr = "0.0.0.0:9898"
broker_endpoint = "http://%s-storage-broker:50051"
pg_distrib_dir='/usr/local/'
[remote_storage]
bucket_name = "%s"
bucket_region = "%s"
prefix_in_bucket = "pageserver"
endpoint = "%s"
`,
		pageserver.Spec.Cluster,
		pageserver.Spec.Cluster,
		bucketName,
		awsRegion,
		awsEndpointURL,
	)

	return &corev1.ConfigMap{
		TypeMeta: metav1.TypeMeta{
			APIVersion: "v1",
			Kind:       "ConfigMap",
		},
		ObjectMeta: metav1.ObjectMeta{
			Name:      configMapName,
			Namespace: pageserver.Namespace,
			Labels: map[string]string{
				"molnett.org/cluster":    pageserver.Spec.Cluster,
				"molnett.org/component":  "pageserver",
				"molnett.org/pageserver": pageserver.Name,
			},
		},
		Data: map[string]string{
			"pageserver.toml": pageserverToml,
		},
	}
}
