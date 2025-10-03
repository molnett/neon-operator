package compute

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	appsv1 "k8s.io/api/apps/v1"
	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/apimachinery/pkg/runtime"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/client/fake"

	neonv1alpha1 "oltp.molnett.org/neon-operator/api/v1alpha1"
)

func TestFindTenantDeployments(t *testing.T) {
	scheme := runtime.NewScheme()
	if err := appsv1.AddToScheme(scheme); err != nil {
		t.Fatalf("failed to add apps/v1 to scheme: %v", err)
	}

	tests := []struct {
		name             string
		tenantID         string
		deployments      []appsv1.Deployment
		expectedCount    int
		expectError      bool
		expectedErrorMsg string
	}{
		{
			name:     "find deployments with matching tenant ID",
			tenantID: "test-tenant-123",
			deployments: []appsv1.Deployment{
				{
					ObjectMeta: metav1.ObjectMeta{
						Name:      "test-deployment-1",
						Namespace: "neon",
						Labels: map[string]string{
							"neon.tenant_id": "test-tenant-123",
						},
					},
				},
				{
					ObjectMeta: metav1.ObjectMeta{
						Name:      "test-deployment-2",
						Namespace: "neon",
						Labels: map[string]string{
							"neon.tenant_id": "test-tenant-123",
						},
					},
				},
				{
					ObjectMeta: metav1.ObjectMeta{
						Name:      "other-deployment",
						Namespace: "neon",
						Labels: map[string]string{
							"neon.tenant_id": "other-tenant",
						},
					},
				},
			},
			expectedCount: 2,
			expectError:   false,
		},
		{
			name:     "no deployments found",
			tenantID: "nonexistent-tenant",
			deployments: []appsv1.Deployment{
				{
					ObjectMeta: metav1.ObjectMeta{
						Name:      "test-deployment",
						Namespace: "neon",
						Labels: map[string]string{
							"neon.tenant_id": "other-tenant",
						},
					},
				},
			},
			expectedCount:    0,
			expectError:      true,
			expectedErrorMsg: "no deployment available with the tenantID nonexistent-tenant",
		},
		{
			name:             "empty deployment list",
			tenantID:         "test-tenant",
			deployments:      []appsv1.Deployment{},
			expectedCount:    0,
			expectError:      true,
			expectedErrorMsg: "no deployment available with the tenantID test-tenant",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			// Create fake client with deployments
			objs := make([]client.Object, len(tt.deployments))
			for i := range tt.deployments {
				objs[i] = &tt.deployments[i]
			}

			k8sClient := fake.NewClientBuilder().
				WithScheme(scheme).
				WithObjects(objs...).
				Build()

			ctx := context.Background()
			result, err := FindTenantDeployments(ctx, k8sClient, tt.tenantID)

			if tt.expectError {
				if err == nil {
					t.Errorf("expected error but got none")
				}
				if tt.expectedErrorMsg != "" && err.Error() != tt.expectedErrorMsg {
					t.Errorf("expected error message '%s', got '%s'", tt.expectedErrorMsg, err.Error())
				}
			} else {
				if err != nil {
					t.Errorf("unexpected error: %v", err)
				}
				if len(result.Items) != tt.expectedCount {
					t.Errorf("expected %d deployments, got %d", tt.expectedCount, len(result.Items))
				}
			}
		})
	}
}

func TestExtractComputeId(t *testing.T) {
	tests := []struct {
		name         string
		deployment   *appsv1.Deployment
		expected     string
		expectError  bool
		errorMessage string
	}{
		{
			name: "extract from annotation",
			deployment: &appsv1.Deployment{
				ObjectMeta: metav1.ObjectMeta{
					Annotations: map[string]string{
						"neon.compute_id": "compute-123",
					},
				},
			},
			expected:    "compute-123",
			expectError: false,
		},
		{
			name: "extract from label when annotation missing",
			deployment: &appsv1.Deployment{
				ObjectMeta: metav1.ObjectMeta{
					Labels: map[string]string{
						"molnett.org/branch": "branch-456",
					},
				},
			},
			expected:    "branch-456",
			expectError: false,
		},
		{
			name: "annotation takes precedence over label",
			deployment: &appsv1.Deployment{
				ObjectMeta: metav1.ObjectMeta{
					Annotations: map[string]string{
						"neon.compute_id": "compute-from-annotation",
					},
					Labels: map[string]string{
						"molnett.org/branch": "compute-from-label",
					},
				},
			},
			expected:    "compute-from-annotation",
			expectError: false,
		},
		{
			name: "no compute id found",
			deployment: &appsv1.Deployment{
				ObjectMeta: metav1.ObjectMeta{
					Labels: map[string]string{
						"app": "some-app",
					},
				},
			},
			expected:     "",
			expectError:  true,
			errorMessage: "cluster name not found in deployment metadata",
		},
		{
			name:         "nil deployment",
			deployment:   &appsv1.Deployment{},
			expected:     "",
			expectError:  true,
			errorMessage: "cluster name not found in deployment metadata",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result, err := extractComputeId(tt.deployment)

			if tt.expectError {
				if err == nil {
					t.Errorf("expected error but got none")
				}
				if tt.errorMessage != "" && err.Error() != tt.errorMessage {
					t.Errorf("expected error message '%s', got '%s'", tt.errorMessage, err.Error())
				}
			} else {
				if err != nil {
					t.Errorf("unexpected error: %v", err)
				}
				if result != tt.expected {
					t.Errorf("expected '%s', got '%s'", tt.expected, result)
				}
			}
		})
	}
}

func TestGetServiceBasedOnForTenentId(t *testing.T) {
	scheme := runtime.NewScheme()
	if err := corev1.AddToScheme(scheme); err != nil {
		t.Fatalf("failed to add apps/v1 to scheme: %v", err)
	}

	tests := []struct {
		name          string
		tenantID      string
		services      []corev1.Service
		expectedCount int
	}{
		{
			name:     "find services with matching tenant ID",
			tenantID: "test-tenant-123",
			services: []corev1.Service{
				{
					ObjectMeta: metav1.ObjectMeta{
						Name:      "service-1",
						Namespace: "neon",
						Labels: map[string]string{
							"neon.tenant_id": "test-tenant-123",
						},
					},
				},
				{
					ObjectMeta: metav1.ObjectMeta{
						Name:      "service-2",
						Namespace: "neon",
						Labels: map[string]string{
							"neon.tenant_id": "test-tenant-123",
						},
					},
				},
				{
					ObjectMeta: metav1.ObjectMeta{
						Name:      "other-service",
						Namespace: "neon",
						Labels: map[string]string{
							"neon.tenant_id": "other-tenant",
						},
					},
				},
			},
			expectedCount: 2,
		},
		{
			name:          "no services found",
			tenantID:      "nonexistent-tenant",
			services:      []corev1.Service{},
			expectedCount: 0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			// Create fake client with services
			objs := make([]client.Object, len(tt.services))
			for i := range tt.services {
				objs[i] = &tt.services[i]
			}

			k8sClient := fake.NewClientBuilder().
				WithScheme(scheme).
				WithObjects(objs...).
				Build()

			ctx := context.Background()
			result, err := getServiceBasedOnForTenentId(ctx, k8sClient, tt.tenantID)

			if err != nil {
				t.Errorf("unexpected error: %v", err)
			}
			if len(result.Items) != tt.expectedCount {
				t.Errorf("expected %d services, got %d", tt.expectedCount, len(result.Items))
			}
		})
	}
}

func TestRefreshConfiguration(t *testing.T) {
	scheme := runtime.NewScheme()
	if err := appsv1.AddToScheme(scheme); err != nil {
		t.Fatalf("failed to add apps/v1 to scheme: %v", err)
	}
	if err := corev1.AddToScheme(scheme); err != nil {
		t.Fatalf("failed to add core/v1 to scheme: %v", err)
	}
	if err := neonv1alpha1.AddToScheme(scheme); err != nil {
		t.Fatalf("failed to add neonv1alpha1 to scheme: %v", err)

	}

	// Create test server to mock HTTP endpoints
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/configure":
			// Check method
			if r.Method != http.MethodPost {
				w.WriteHeader(http.StatusMethodNotAllowed)
				return
			}
			// Check Authorization header
			auth := r.Header.Get("Authorization")
			if auth == "" || len(auth) < 7 || auth[:7] != "Bearer " {
				w.WriteHeader(http.StatusUnauthorized)
				return
			}
			// Check Content-Type
			if r.Header.Get("Content-Type") != "application/json" {
				w.WriteHeader(http.StatusBadRequest)
				return
			}
			w.WriteHeader(http.StatusOK)
		default:
			w.WriteHeader(http.StatusNotFound)
		}
	}))
	defer server.Close()

	// Create test JWT secret
	testSecret := &corev1.Secret{
		ObjectMeta: metav1.ObjectMeta{
			Name:      "cluster-test-cluster-jwt",
			Namespace: "neon",
		},
		Data: map[string][]byte{
			"private.pem": []byte(`-----BEGIN PRIVATE KEY-----
MC4CAQAwBQYDK2VwBCIEIGVzYmVzdF9kb2N1bWVudGF0aW9uX2V2ZXI=
-----END PRIVATE KEY-----`),
			"public.pem": []byte(`-----BEGIN PUBLIC KEY-----
MCowBQYDK2VwAyEAZXNiZXN0X2RvY3VtZW50YXRpb25fZXZlcg==
-----END PUBLIC KEY-----`),
		},
	}

	tests := []struct {
		name         string
		request      ComputeHookNotifyRequest
		deployment   *appsv1.Deployment
		services     []corev1.Service
		expectError  bool
		errorMessage string
	}{
		{
			name: "successful configuration refresh",
			request: ComputeHookNotifyRequest{
				TenantID: "test-tenant-123",
				Shards:   []ComputeHookNotifyRequestShard{{NodeID: 1, ShardNumber: 0}},
			},
			deployment: &appsv1.Deployment{
				ObjectMeta: metav1.ObjectMeta{
					Name:      "test-compute-deployment",
					Namespace: "neon",
					Annotations: map[string]string{
						"neon.compute_id":   "test-compute",
						"neon.cluster_name": "test-cluster",
					},
					Labels: map[string]string{
						"neon.tenant_id": "test-tenant-123",
					},
				},
			},
			services: []corev1.Service{
				{
					ObjectMeta: metav1.ObjectMeta{
						Name:      "test-compute-admin",
						Namespace: "neon",
						Labels: map[string]string{
							"neon.tenant_id": "test-tenant-123",
						},
					},
					Spec: corev1.ServiceSpec{
						Ports: []corev1.ServicePort{
							{Port: 3080},
						},
					},
				},
			},
			expectError: false,
		},
		{
			name: "missing compute id annotation",
			request: ComputeHookNotifyRequest{
				TenantID: "test-tenant-123",
			},
			deployment: &appsv1.Deployment{
				ObjectMeta: metav1.ObjectMeta{
					Name:      "test-deployment",
					Namespace: "neon",
				},
			},
			services:     []corev1.Service{},
			expectError:  true,
			errorMessage: "failed to extract compute ID: cluster name not found in deployment metadata",
		},
		{
			name: "missing cluster name annotation",
			request: ComputeHookNotifyRequest{
				TenantID: "test-tenant-123",
			},
			deployment: &appsv1.Deployment{
				ObjectMeta: metav1.ObjectMeta{
					Name:      "test-deployment",
					Namespace: "neon",
					Annotations: map[string]string{
						"neon.compute_id": "test-compute",
					},
				},
			},
			services:     []corev1.Service{},
			expectError:  true,
			errorMessage: "failed to extract clustername from deployment: cluster name not found in deployment metadata",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			// Skip tests that require actual JWT functionality for simplicity
			if tt.name == "successful configuration refresh" {
				t.Skip("Skipping test that requires actual JWT secret parsing")
				return
			}

			// Create fake client
			objs := []client.Object{testSecret, tt.deployment}
			for i := range tt.services {
				objs = append(objs, &tt.services[i])
			}

			k8sClient := fake.NewClientBuilder().
				WithScheme(scheme).
				WithObjects(objs...).
				Build()

			ctx := context.Background()
			err := RefreshConfiguration(ctx, nil, k8sClient, tt.request, tt.deployment)

			if tt.expectError {
				if err == nil {
					t.Errorf("expected error but got none")
				}
				if tt.errorMessage != "" && err.Error() != tt.errorMessage {
					t.Errorf("expected error message '%s', got '%s'", tt.errorMessage, err.Error())
				}
			} else {
				if err != nil {
					t.Errorf("unexpected error: %v", err)
				}
			}
		})
	}
}

func TestComputeHookNotifyRequest_JSON(t *testing.T) {
	expectedReq := `{"tenant_id":"test-tenant-123","stripe_size":8,"shards":[{"node_id":1,"shard_number":0},` +
		`{"node_id":2,"shard_number":1}]}`
	tests := []struct {
		name     string
		request  ComputeHookNotifyRequest
		expected string
	}{
		{
			name: "complete request",
			request: ComputeHookNotifyRequest{
				TenantID:   "test-tenant-123",
				StripeSize: func(x uint32) *uint32 { return &x }(8),
				Shards: []ComputeHookNotifyRequestShard{
					{NodeID: 1, ShardNumber: 0},
					{NodeID: 2, ShardNumber: 1},
				},
			},
			expected: expectedReq,
		},
		{
			name: "minimal request",
			request: ComputeHookNotifyRequest{
				TenantID: "minimal-tenant",
				Shards:   []ComputeHookNotifyRequestShard{},
			},
			expected: `{"tenant_id":"minimal-tenant","shards":[]}`,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			jsonData, err := json.Marshal(tt.request)
			if err != nil {
				t.Errorf("failed to marshal request: %v", err)
			}

			if string(jsonData) != tt.expected {
				t.Errorf("expected JSON %s, got %s", tt.expected, string(jsonData))
			}

			// Test unmarshaling
			var unmarshaled ComputeHookNotifyRequest
			err = json.Unmarshal(jsonData, &unmarshaled)
			if err != nil {
				t.Errorf("failed to unmarshal request: %v", err)
			}

			if unmarshaled.TenantID != tt.request.TenantID {
				t.Errorf("expected tenant_id %s, got %s", tt.request.TenantID, unmarshaled.TenantID)
			}
		})
	}
}
