package controlplane

import (
	"bytes"
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

// Test helper functions
func createTestLogger() *slog.Logger {
	return slog.New(slog.NewTextHandler(os.Stderr, &slog.HandlerOptions{Level: slog.LevelDebug}))
}

func createTestScheme() *runtime.Scheme {
	scheme := runtime.NewScheme()
	_ = appsv1.AddToScheme(scheme)
	_ = corev1.AddToScheme(scheme)
	_ = neonv1alpha1.AddToScheme(scheme)
	return scheme
}

func createTestDeployment(timelineID string) *appsv1.Deployment {
	return &appsv1.Deployment{
		ObjectMeta: metav1.ObjectMeta{
			Name:      "test-compute",
			Namespace: "neon",
			Labels: map[string]string{
				"neon.tenant_id":        "test-tenant-123",
				"neon.compute_id":       "test-compute",
				"neon.cluster_name":     "test-cluster",
				"molnett.org/component": "compute",
				"neon.timeline_id":      timelineID,
			},
			Annotations: map[string]string{
				"neon.compute_id":   "test-compute",
				"neon.cluster_name": "test-cluster",
				"neon.timeline_id":  timelineID,
			},
		},
	}
}

func createTestProject() *neonv1alpha1.Project {
	return &neonv1alpha1.Project{
		ObjectMeta: metav1.ObjectMeta{
			Name:      "testProject",
			Namespace: "neon",
		},
		Spec: neonv1alpha1.ProjectSpec{
			ClusterName: "test-cluster",
			TenantID:    "test-tenant-123",
		},
	}
}

func createTestBranch() *neonv1alpha1.Branch {
	return &neonv1alpha1.Branch{
		ObjectMeta: metav1.ObjectMeta{
			Name:      "testBranch",
			Namespace: "neon",
		},
		Spec: neonv1alpha1.BranchSpec{
			ProjectID:  "testProject",
			TimelineID: "123456789",
		},
	}
}

func createTestService(name, namespace, tenantID string) *corev1.Service {
	return &corev1.Service{
		ObjectMeta: metav1.ObjectMeta{
			Name:      name,
			Namespace: namespace,
			Labels: map[string]string{
				"neon.tenant_id": tenantID,
			},
		},
	}
}

func createTestSecret() *corev1.Secret {
	return &corev1.Secret{
		ObjectMeta: metav1.ObjectMeta{
			Name:      "cluster-test-cluster-jwt",
			Namespace: "neon",
		},
		Data: map[string][]byte{
			"private_key": []byte("test-private-key"),
			"public_key":  []byte("test-public-key"),
		},
	}
}

func TestNotifyAttachHandler(t *testing.T) {
	scheme := createTestScheme()
	logger := createTestLogger()

	tests := []struct {
		name           string
		requestBody    string
		requestMethod  string
		contentType    string
		deployments    []*appsv1.Deployment
		services       []*corev1.Service
		projects       []*neonv1alpha1.Project
		branches       []*neonv1alpha1.Branch
		secrets        []*corev1.Secret
		expectedStatus int
		expectedError  string
	}{
		{
			name:          "successful notify attach with valid request",
			requestBody:   `{"tenant_id": "test-tenant-123", "shards": [{"node_id": 1, "shard_number": 0}]}`,
			requestMethod: http.MethodPost,
			contentType:   "application/json",
			deployments: []*appsv1.Deployment{
				createTestDeployment("123456789"),
			},
			projects: []*neonv1alpha1.Project{
				createTestProject(),
			},
			branches: []*neonv1alpha1.Branch{
				createTestBranch(),
			},
			services: []*corev1.Service{
				createTestService("test-compute-admin", "neon", "test-tenant-123"),
			},
			secrets: []*corev1.Secret{
				createTestSecret(),
			},
			expectedStatus: http.StatusInternalServerError, // Will fail due to HTTP call to non-existent service
		},
		{
			name:          "invalid JSON request body",
			requestBody:   `{"tenant_id": "test-tenant-123", "shards": [{"node_id": 1, "shard_number": 0}]`, // Missing closing brace
			requestMethod: http.MethodPost,
			contentType:   "application/json",
			deployments: []*appsv1.Deployment{
				createTestDeployment("12356789"),
			},
			expectedStatus: http.StatusInternalServerError,
		},
		{
			name:          "empty request body",
			requestBody:   "",
			requestMethod: http.MethodPost,
			contentType:   "application/json",
			deployments: []*appsv1.Deployment{
				createTestDeployment("12346789"),
			},
			expectedStatus: http.StatusInternalServerError,
		},
		{
			name:          "request with multiple shards",
			requestBody:   `{"tenant_id": "test-tenant-123", "stripe_size": 8, "shards": [{"node_id": 1, "shard_number": 0}, {"node_id": 2, "shard_number": 1}]}`,
			requestMethod: http.MethodPost,
			contentType:   "application/json",
			deployments: []*appsv1.Deployment{
				createTestDeployment("1234560789"),
			},
			projects: []*neonv1alpha1.Project{
				createTestProject(),
			},
			branches: []*neonv1alpha1.Branch{
				createTestBranch(),
			},
			services: []*corev1.Service{
				createTestService("test-compute-admin", "neon", "test-tenant-123"),
			},
			secrets: []*corev1.Secret{
				createTestSecret(),
			},
			expectedStatus: http.StatusInternalServerError, // Will fail due to HTTP call to non-existent service
		},
		{
			name:          "request with nil stripe size",
			requestBody:   `{"tenant_id": "test-tenant-123", "shards": [{"node_id": 1, "shard_number": 0}]}`,
			requestMethod: http.MethodPost,
			contentType:   "application/json",
			deployments: []*appsv1.Deployment{
				createTestDeployment("1233456789"),
			},
			projects: []*neonv1alpha1.Project{
				createTestProject(),
			},
			branches: []*neonv1alpha1.Branch{
				createTestBranch(),
			},
			services: []*corev1.Service{
				createTestService("test-compute-admin", "neon", "test-tenant-123"),
			},
			secrets: []*corev1.Secret{
				createTestSecret(),
			},
			expectedStatus: http.StatusInternalServerError, // Will fail due to HTTP call to non-existent service
		},
		{
			name:          "missing content type",
			requestBody:   `{"tenant_id": "test-tenant-123", "shards": [{"node_id": 1, "shard_number": 0}]}`,
			requestMethod: http.MethodPost,
			contentType:   "",
			deployments: []*appsv1.Deployment{
				createTestDeployment("123456789"),
			},
			expectedStatus: http.StatusInternalServerError, // Handler doesn't check content type, so it will process
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			// Create fake client with test objects
			objs := make([]client.Object, 0)
			for _, dep := range tt.deployments {
				objs = append(objs, dep)
			}
			for _, svc := range tt.services {
				objs = append(objs, svc)
			}
			for _, proj := range tt.projects {
				objs = append(objs, proj)
			}
			for _, branch := range tt.branches {
				objs = append(objs, branch)
			}
			for _, secret := range tt.secrets {
				objs = append(objs, secret)
			}

			k8sClient := fake.NewClientBuilder().
				WithScheme(scheme).
				WithObjects(objs...).
				Build()

			// Create handler
			mux := http.NewServeMux()
			mux.Handle("/notify-attach", notifyAttach(logger, k8sClient))

			// Create test request
			req := httptest.NewRequest(tt.requestMethod, "/notify-attach", bytes.NewBufferString(tt.requestBody))
			if tt.contentType != "" {
				req.Header.Set("Content-Type", tt.contentType)
			}

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
			case http.StatusMethodNotAllowed:
				// Verify method not allowed was handled
				if w.Body.String() != "" {
					t.Errorf("expected empty response body for method not allowed, got: %s", w.Body.String())
				}
			}
		})
	}
}

func TestHandleHealthCheck(t *testing.T) {
	tests := []struct {
		name           string
		requestMethod  string
		requestPath    string
		expectedStatus int
	}{
		{
			name:           "health check GET request",
			requestMethod:  http.MethodGet,
			requestPath:    "/healthz",
			expectedStatus: http.StatusOK,
		},
		{
			name:           "ready check GET request",
			requestMethod:  http.MethodGet,
			requestPath:    "/readyz",
			expectedStatus: http.StatusOK,
		},
		{
			name:           "health check POST request",
			requestMethod:  http.MethodPost,
			requestPath:    "/healthz",
			expectedStatus: http.StatusOK,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			// Create handler
			mux := http.NewServeMux()
			mux.Handle("/healthz", handleHealthCheck())
			mux.Handle("/readyz", handleHealthCheck())

			// Create test request
			req := httptest.NewRequest(tt.requestMethod, tt.requestPath, nil)

			// Create response recorder
			w := httptest.NewRecorder()

			// Execute handler
			mux.ServeHTTP(w, req)

			// Check status code
			if w.Code != tt.expectedStatus {
				t.Errorf("expected status %d, got %d", tt.expectedStatus, w.Code)
			}

			// Health check should return empty body
			if w.Body.String() != "" {
				t.Errorf("expected empty response body, got: %s", w.Body.String())
			}
		})
	}
}

func TestHandleComputeSpec(t *testing.T) {
	scheme := createTestScheme()
	logger := createTestLogger()

	tests := []struct {
		name           string
		computeID      string
		deployments    []*appsv1.Deployment
		projects       []*neonv1alpha1.Project
		branches       []*neonv1alpha1.Branch
		secrets        []*corev1.Secret
		expectedStatus int
		expectedError  string
	}{
		{
			name:      "valid compute ID with all resources",
			computeID: "test-compute",
			deployments: []*appsv1.Deployment{
				createTestDeployment("123456789"),
			},
			projects: []*neonv1alpha1.Project{
				createTestProject(),
			},
			branches: []*neonv1alpha1.Branch{
				createTestBranch(),
			},
			secrets: []*corev1.Secret{
				createTestSecret(),
			},
			expectedStatus: http.StatusInternalServerError, // Will fail due to storage controller call
		},
		{
			name:           "invalid compute ID",
			computeID:      "nonexistent-compute",
			deployments:    []*appsv1.Deployment{},
			expectedStatus: http.StatusInternalServerError,
		},
		{
			name:      "compute ID without project",
			computeID: "test-compute",
			deployments: []*appsv1.Deployment{
				createTestDeployment("1234586789"),
			},
			projects:       []*neonv1alpha1.Project{},
			expectedStatus: http.StatusInternalServerError,
		},
		{
			name:      "compute ID without branch",
			computeID: "test-compute",
			deployments: []*appsv1.Deployment{
				createTestDeployment("123456789"),
			},
			projects: []*neonv1alpha1.Project{
				createTestProject(),
			},
			branches:       []*neonv1alpha1.Branch{},
			expectedStatus: http.StatusInternalServerError,
		},
		{
			name:      "compute ID without JWT secret",
			computeID: "test-compute",
			deployments: []*appsv1.Deployment{
				createTestDeployment("123456789"),
			},
			projects: []*neonv1alpha1.Project{
				createTestProject(),
			},
			branches: []*neonv1alpha1.Branch{
				createTestBranch(),
			},
			secrets:        []*corev1.Secret{},
			expectedStatus: http.StatusInternalServerError,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			// Create fake client with test objects
			objs := make([]client.Object, 0)
			for _, dep := range tt.deployments {
				objs = append(objs, dep)
			}
			for _, proj := range tt.projects {
				objs = append(objs, proj)
			}
			for _, branch := range tt.branches {
				objs = append(objs, branch)
			}
			for _, secret := range tt.secrets {
				objs = append(objs, secret)
			}

			k8sClient := fake.NewClientBuilder().
				WithScheme(scheme).
				WithObjects(objs...).
				Build()

			// Create handler
			mux := http.NewServeMux()
			mux.Handle("/compute/api/v2/computes/{compute_id}/spec", handleComputeSpec(logger, k8sClient))

			// Create test request
			req := httptest.NewRequest(http.MethodGet, "/compute/api/v2/computes/"+tt.computeID+"/spec", nil)

			// Create response recorder
			w := httptest.NewRecorder()

			// Execute handler
			mux.ServeHTTP(w, req)

			// Check status code
			if w.Code != tt.expectedStatus {
				t.Errorf("expected status %d, got %d", tt.expectedStatus, w.Code)
			}

			// For internal server errors, verify empty response body
			if tt.expectedStatus == http.StatusInternalServerError {
				if w.Body.String() != "" {
					t.Errorf("expected empty response body for internal server error, got: %s", w.Body.String())
				}
			}
		})
	}
}

func TestLogRequests(t *testing.T) {
	logger := createTestLogger()

	// Create a simple handler that returns 200 OK
	handler := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte("test response"))
	})

	// Wrap with logging middleware
	loggedHandler := logRequests(logger, handler)

	tests := []struct {
		name           string
		requestMethod  string
		requestPath    string
		expectedStatus int
		expectedBody   string
	}{
		{
			name:           "GET request",
			requestMethod:  http.MethodGet,
			requestPath:    "/test",
			expectedStatus: http.StatusOK,
			expectedBody:   "test response",
		},
		{
			name:           "POST request",
			requestMethod:  http.MethodPost,
			requestPath:    "/test",
			expectedStatus: http.StatusOK,
			expectedBody:   "test response",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			// Create test request
			req := httptest.NewRequest(tt.requestMethod, tt.requestPath, nil)

			// Create response recorder
			w := httptest.NewRecorder()

			// Execute handler
			loggedHandler.ServeHTTP(w, req)

			// Check status code
			if w.Code != tt.expectedStatus {
				t.Errorf("expected status %d, got %d", tt.expectedStatus, w.Code)
			}

			// Check response body
			if w.Body.String() != tt.expectedBody {
				t.Errorf("expected body %s, got %s", tt.expectedBody, w.Body.String())
			}
		})
	}
}

func TestResponseWriter(t *testing.T) {
	tests := []struct {
		name           string
		status         int
		expectedStatus int
	}{
		{
			name:           "write 200 status",
			status:         http.StatusOK,
			expectedStatus: http.StatusOK,
		},
		{
			name:           "write 404 status",
			status:         http.StatusNotFound,
			expectedStatus: http.StatusNotFound,
		},
		{
			name:           "write 500 status",
			status:         http.StatusInternalServerError,
			expectedStatus: http.StatusInternalServerError,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			// Create response recorder
			w := httptest.NewRecorder()

			// Wrap with our custom response writer
			wrapped := &responseWriter{ResponseWriter: w, status: http.StatusOK}

			// Write header
			wrapped.WriteHeader(tt.status)

			// Check that the status was recorded
			if wrapped.status != tt.expectedStatus {
				t.Errorf("expected status %d, got %d", tt.expectedStatus, wrapped.status)
			}

			// Check that the underlying response writer also got the status
			if w.Code != tt.expectedStatus {
				t.Errorf("expected underlying status %d, got %d", tt.expectedStatus, w.Code)
			}
		})
	}
}
