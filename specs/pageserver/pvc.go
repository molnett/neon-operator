package pageserver

import (
	"fmt"

	corev1 "k8s.io/api/core/v1"
	"k8s.io/apimachinery/pkg/api/resource"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"oltp.molnett.org/neon-operator/api/v1alpha1"
)

func PersistentVolumeClaim(pageserver *v1alpha1.Pageserver) *corev1.PersistentVolumeClaim {
	pvcName := fmt.Sprintf("%s-pageserver-%d", pageserver.Spec.Cluster, pageserver.Spec.ID)
	pvc := &corev1.PersistentVolumeClaim{
		TypeMeta: metav1.TypeMeta{
			APIVersion: "v1",
			Kind:       "PersistentVolumeClaim",
		},
		ObjectMeta: metav1.ObjectMeta{
			Name:      pvcName,
			Namespace: pageserver.Namespace,
			Labels: map[string]string{
				"molnett.org/cluster":    pageserver.Spec.Cluster,
				"molnett.org/component":  "pageserver",
				"molnett.org/pageserver": pageserver.Name,
			},
		},
		Spec: corev1.PersistentVolumeClaimSpec{
			AccessModes: []corev1.PersistentVolumeAccessMode{
				corev1.ReadWriteOnce,
			},
			Resources: corev1.VolumeResourceRequirements{
				Requests: corev1.ResourceList{
					corev1.ResourceStorage: resource.MustParse(pageserver.Spec.StorageConfig.Size),
				},
			},
		},
	}

	if pageserver.Spec.StorageConfig.StorageClass != nil {
		pvc.Spec.StorageClassName = pageserver.Spec.StorageConfig.StorageClass
	}

	return pvc
}
