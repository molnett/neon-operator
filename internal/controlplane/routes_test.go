package controlplane

import (
	"bytes"
	"context"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"os"
	"testing"

	appsv1 "k8s.io/api/apps/v1"
	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/apimachinery/pkg/runtime"
	neonv1alpha1 "oltp.molnett.org/neon-operator/api/v1alpha1"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/client/fake"
)

func TestCheckTenantIDInDeployments(t *testing.T) {
	scheme := runtime.NewScheme()
	if err := appsv1.AddToScheme(scheme); err != nil {
		t.Fatalf("failed to add apps/v1 to scheme: %v", err)
	}

	tests := []struct {
		name           string
		tenantID       string
		deployments    []appsv1.Deployment
		expectedExists bool
		expectError    bool
		errorMessage   string
	}{
		{
			name:     "tenant ID exists in deployment labels",
			tenantID: "test-tenant-123",
			deployments: []appsv1.Deployment{
				{
					ObjectMeta: metav1.ObjectMeta{
						Name:      "test-deployment-1",
						Namespace: "neon",
						Labels: map[string]string{
							"neon.tenant_id":        "test-tenant-123",
							"molnett.org/component": "compute",
							"neon.compute_id":       "test-compute",
						},
					},
				},
			},
			expectedExists: true,
			expectError:    false,
		},
		{
			name:     "tenant ID does not exist",
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
			expectedExists: false,
			expectError:    true,
			errorMessage:   "failed to find deployments with tenantID nonexistent-tenant: no deployment available with the tenantID nonexistent-tenant",
		},
		{
			name:           "no deployments",
			tenantID:       "test-tenant",
			deployments:    []appsv1.Deployment{},
			expectedExists: false,
			expectError:    true,
			errorMessage:   "failed to find deployments with tenantID test-tenant: no deployment available with the tenantID test-tenant",
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
			exists, err := checkTenantIDInDeployments(ctx, k8sClient, tt.tenantID)

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
				if exists != tt.expectedExists {
					t.Errorf("expected exists=%v, got %v", tt.expectedExists, exists)
				}
			}
		})
	}
}

func TestNotifyAttachHandler(t *testing.T) {
	scheme := runtime.NewScheme()
	if err := appsv1.AddToScheme(scheme); err != nil {
		t.Fatalf("failed to add apps/v1 to scheme: %v", err)
	}

	if err := corev1.AddToScheme(scheme); err != nil {
		t.Fatalf("failed to add apps/v1 to scheme: %v", err)
	}
	if err := neonv1alpha1.AddToScheme(scheme); err != nil {

		t.Fatalf("failed to add neonv1alpha1 to scheme: %v", err)
	}
	tests := []struct {
		name              string
		requestBody       string
		deployments       []appsv1.Deployment
		services          []corev1.Service
		projects          []neonv1alpha1.Project
		expectedStatus    int
		shouldCallRefresh bool
	}{
		{
			name: "successful notify attach",
			requestBody: `{
				"tenant_id": "test-tenant-123",
				"shards": [{"node_id": 1, "shard_number": 0}]
			}`,
			deployments: []appsv1.Deployment{
				{
					ObjectMeta: metav1.ObjectMeta{
						Name:      "test-compute",
						Namespace: "neon",
						Labels: map[string]string{
							"neon.tenant_id":        "test-tenant-123",
							"neon.compute_id":       "test-compute",
							"neon.cluster_name":     "test-cluster",
							"molnett.org/component": "compute",
							"neon.timeline_id":      "123456789",
						},
						Annotations: map[string]string{
							"neon.compute_id":   "test-compute",
							"neon.cluster_name": "test-cluster",
							"neon.timeline_id":  "123456789",
						},
					},
				},
			},
			projects: []neonv1alpha1.Project{
				{
					ObjectMeta: metav1.ObjectMeta{
						Name:      "testProject",
						Namespace: "neon",
					},
					Spec: neonv1alpha1.ProjectSpec{
						ClusterName: "test-cluster",
						TenantID:    "123456789",
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
				},
			},
			expectedStatus:    http.StatusInternalServerError,
			shouldCallRefresh: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			// Create fake client
			objs := make([]client.Object, 0)
			for i := range tt.deployments {
				objs = append(objs, &tt.deployments[i])
			}
			for i := range tt.services {
				objs = append(objs, &tt.services[i])
			}
			for i := range tt.projects {
				objs = append(objs, &tt.projects[i])
			}

			k8sClient := fake.NewClientBuilder().
				WithScheme(scheme).
				WithObjects(objs...).
				Build()
			logger := slog.New(slog.NewTextHandler(os.Stderr, nil))
			// Create handler
			mux := http.NewServeMux()
			mux.Handle("/notify-attach", notifyAttach(logger, k8sClient))

			// Create test request
			req := httptest.NewRequest(http.MethodPost, "/notify-attach", bytes.NewBufferString(tt.requestBody))
			req.Header.Set("Content-Type", "application/json")

			// Create response recorder
			w := httptest.NewRecorder()

			// Execute handler
			mux.ServeHTTP(w, req)
			// Check status code
			if w.Code != tt.expectedStatus {
				t.Errorf("expected status %d, got %d", tt.expectedStatus, w.Code)
			}

			// Additional checks based on expected status
			switch tt.expectedStatus {
			case http.StatusOK:
				// Verify request was processed successfully
				if w.Body.String() != "" {
					t.Errorf("expected empty response body for successful request, got: %s", w.Body.String())
				}
			case http.StatusNotFound:
				// Verify tenant ID not found was handled
				if w.Body.String() != "" {
					t.Errorf("expected empty response body for not found, got: %s", w.Body.String())
				}
			case http.StatusInternalServerError:
				// Verify internal server error was handled
				if w.Body.String() != "" {
					t.Errorf("expected empty response body for internal server error, got: %s", w.Body.String())
				}
			}
		})
	}
}
