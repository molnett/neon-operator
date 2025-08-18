package v1alpha1

import (
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/utils/ptr"
)

func (cluster *Cluster) GetOwnerReference() *metav1.OwnerReference {
	return &metav1.OwnerReference{
		APIVersion: cluster.APIVersion,
		Kind:       cluster.Kind,
		Name:       cluster.Name,
		UID:        cluster.UID,
		Controller: ptr.To(true),
	}
}
