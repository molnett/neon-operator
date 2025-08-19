package compute

import (
	"encoding/json"
	"fmt"

	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"

	neonv1alpha1 "oltp.molnett.org/neon-operator/api/v1alpha1"
	"oltp.molnett.org/neon-operator/utils"
)

func ConfigMap(branch *neonv1alpha1.Branch, project *neonv1alpha1.Project, jwtSecret corev1.Secret) (*corev1.ConfigMap, error) {

	jwtManager, err := utils.NewJWTManagerFromSecret(&jwtSecret)
	if err != nil {
		return nil, err
	}

	jwk := jwtManager.ToJWK()

	type computeCtlConfig struct {
		JWKS utils.JWKResponse `json:"jwks"`
	}

	type computeSpec struct {
		FormatVersion    string           `json:"format_version"`
		ComputeCtlConfig computeCtlConfig `json:"compute_ctl_config"`
	}

	var spec computeSpec

	spec.FormatVersion = "1.0"
	spec.ComputeCtlConfig.JWKS = *jwk

	specJSON, err := json.Marshal(spec)
	if err != nil {
		return nil, err
	}

	return &corev1.ConfigMap{
		TypeMeta: metav1.TypeMeta{
			APIVersion: "v1",
			Kind:       "ConfigMap",
		},
		ObjectMeta: metav1.ObjectMeta{
			Name:      fmt.Sprintf("%s-compute-spec", branch.Name),
			Namespace: branch.Namespace,
			Labels: map[string]string{
				"molnett.org/cluster":   project.Spec.ClusterName,
				"molnett.org/component": "compute",
				"molnett.org/branch":    branch.Name,
			},
		},
		Data: map[string]string{
			"spec.json": string(specJSON),
		},
	}, nil
}
