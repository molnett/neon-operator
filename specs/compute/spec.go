package compute

import (
	"bytes"
	"context"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"log/slog"
	"net/http"
	"time"

	appsv1 "k8s.io/api/apps/v1"
	corev1 "k8s.io/api/core/v1"
	"k8s.io/apimachinery/pkg/types"

	neonv1alpha1 "oltp.molnett.org/neon-operator/api/v1alpha1"
	"oltp.molnett.org/neon-operator/utils"
	"sigs.k8s.io/controller-runtime/pkg/client"
)

// ShardIndex represents a shard number and count pair
type ShardIndex struct {
	ShardNumber uint8 `json:"shard_number"`
	ShardCount  uint8 `json:"shard_count"`
}

// String returns the hex representation of the shard index
func (s ShardIndex) String() string {
	return fmt.Sprintf("%02x%02x", s.ShardNumber, s.ShardCount)
}

// MarshalJSON implements custom JSON marshaling for ShardIndex
func (s ShardIndex) MarshalJSON() ([]byte, error) {
	return json.Marshal(s.String())
}

// UnmarshalJSON implements custom JSON unmarshaling for ShardIndex
func (s *ShardIndex) UnmarshalJSON(data []byte) error {
	var str string
	if err := json.Unmarshal(data, &str); err != nil {
		return err
	}

	if len(str) != 4 {
		return fmt.Errorf("invalid shard index string length: %d", len(str))
	}

	decodedbytes, err := hex.DecodeString(str)
	if err != nil {
		return err
	}

	s.ShardNumber = decodedbytes[0]
	s.ShardCount = decodedbytes[1]
	return nil
}

// ParseShardIndex parses a hex string into a ShardIndex
func ParseShardIndex(s string) (ShardIndex, error) {
	var si ShardIndex
	err := si.UnmarshalJSON([]byte(fmt.Sprintf(`"%s"`, s)))
	return si, err
}

// PageserverShardConnectionInfo contains connection information for a pageserver shard
type PageserverShardConnectionInfo struct {
	ID       *uint64 `json:"id,omitempty"`
	LibpqURL *string `json:"libpq_url,omitempty"`
	GrpcURL  *string `json:"grpc_url,omitempty"`
}

// PageserverShardInfo contains information about pageserver shards
type PageserverShardInfo struct {
	Pageservers []PageserverShardConnectionInfo `json:"pageservers"`
}

type ComputeHookNotifyRequestShard struct {
	NodeID      uint64 `json:"node_id"`
	ShardNumber uint32 `json:"shard_number"`
}
type ComputeHookNotifyRequest struct {
	TenantID   string                          `json:"tenant_id"`
	StripeSize *uint32                         `json:"stripe_size,omitempty"`
	Shards     []ComputeHookNotifyRequestShard `json:"shards"`
}

// Role represents a database role configuration
type Role struct {
	Name              string      `json:"name"`
	EncryptedPassword string      `json:"encrypted_password"`
	Options           interface{} `json:"options"`
}

type SettingsEntry struct {
	Name    string `json:"name"`
	Value   string `json:"value"`
	Vartype string `json:"vartype"`
}

// ClusterConfig represents cluster configuration in the compute spec
type ClusterConfig struct {
	ClusterID string          `json:"cluster_id"`
	Name      string          `json:"name"`
	Roles     []Role          `json:"roles"`
	Databases []interface{}   `json:"databases"`
	Settings  []SettingsEntry `json:"settings"`
}

// PageserverConnectionInfo represents pageserver connection information
type PageserverConnectionInfo struct {
	ShardCount int                            `json:"shard_count"`
	Shards     map[string]PageserverShardInfo `json:"shards"`
}

// ComputeCtlConfig represents compute control configuration
type ComputeCtlConfig struct {
	JWKS *utils.JWKResponse `json:"jwks"`
}

// ComputeSpec represents the main compute specification
type ComputeSpec struct {
	FormatVersion            float64                  `json:"format_version"`
	SuspendTimeoutSeconds    int                      `json:"suspend_timeout_seconds"`
	Cluster                  ClusterConfig            `json:"cluster"`
	DeltaOperations          []interface{}            `json:"delta_operations"`
	SafekeeperConnstrings    []string                 `json:"safekeeper_connstrings"`
	PageserverConnectionInfo PageserverConnectionInfo `json:"pageserver_connection_info"`
}

// ComputeSpecResponse represents the complete JSON response
type ComputeSpecResponse struct {
	Spec             ComputeSpec      `json:"spec"`
	ComputeCtlConfig ComputeCtlConfig `json:"compute_ctl_config"`
	Status           string           `json:"status"`
}

func RefreshConfiguration(ctx context.Context,
	log *slog.Logger,
	k8sClient client.Client,
	request ComputeHookNotifyRequest,
	deployment *appsv1.Deployment) error {
	var computeId string
	var exists bool
	tenentServices := &corev1.ServiceList{}

	if annotations := deployment.GetAnnotations(); annotations != nil {
		computeId, exists = annotations["neon.compute_id"]
		if !exists {
			return fmt.Errorf("failed to extract compute ID from annotations")
		}
	}

	clusterName, err := extractClusterName(deployment)
	if err != nil {
		return fmt.Errorf("failed to extract clustername from deployment: %w", err)
	}

	spec, err := GenerateComputeSpec(ctx, log, k8sClient, &request, computeId)
	if err != nil {
		return fmt.Errorf("failed to generate compute spec: %w", err)
	}
	specBytes, err := json.Marshal(spec)
	if err != nil {
		return fmt.Errorf("failed to marshal spec JSON: %w", err)
	}

	// Get JWT manager from secret to generate tokens
	secretName := fmt.Sprintf("cluster-%s-jwt", clusterName)
	secret := &corev1.Secret{}
	if err := k8sClient.Get(ctx, client.ObjectKey{Name: secretName, Namespace: "neon"}, secret); err != nil {
		return fmt.Errorf("failed to get JWT secret %s: %w", secretName, err)
	}

	jwtManager, err := utils.NewJWTManagerFromSecret(secret)
	if err != nil {
		return fmt.Errorf("failed to create JWT manager from secret: %w", err)
	}

	log.Info("Successfully created JWT manager")

	// Create JWT claims
	now := time.Now()
	claims := map[string]any{
		"compute_id": computeId,
		"aud":        "compute",
		"roles":      []string{"compute_ctl:admin"},
		"exp":        now.Add(1 * time.Hour).Unix(), // 1 hour expiry
		"iat":        now.Unix(),                    // issued at
		"iss":        "neon-operator",               // issuer
		"sub":        computeId,                     // subject
	}

	// Generate the signed JWT token
	tokenString, err := jwtManager.GenerateToken(claims)
	if err != nil {
		return fmt.Errorf("failed to generate JWT token: %w", err)
	}

	log.Info("Successfully generated JWT token", "compute_id", computeId)

	err = k8sClient.List(ctx, tenentServices, client.MatchingLabels{
		"neon.tenant_id": request.TenantID})
	if err != nil {
		return fmt.Errorf("failed to get service based on tenant ID: %w", err)
	}
	for _, service := range tenentServices.Items {
		serviceName := service.Name
		serviceNamespace := service.Namespace
		expectedServiceName := computeId + "-admin"
		if expectedServiceName == serviceName {
			url := fmt.Sprintf("http://%s-admin.%s:3080/configure", computeId, serviceNamespace)
			log.Info("Calling /configure endpoint at URL: ", "URL is", url)
			// Create HTTP request
			req, err := http.NewRequestWithContext(context.Background(), http.MethodPost, url, bytes.NewBuffer(specBytes))
			if err != nil {
				return fmt.Errorf("failed to create request for service %s: %w", serviceName, err)
			}
			req.Header.Set("Authorization", fmt.Sprintf("Bearer %s", tokenString))
			req.Header.Set("Content-Type", "application/json")
			// HTTP client with timeout
			computeClient := &http.Client{Timeout: 2 * time.Second}
			resp, err := computeClient.Do(req)
			if err != nil {
				log.Info("Failed to call /configure ", "service", serviceName, "URL", url, "error", err)
				return fmt.Errorf("failed to call /configure for service %s: %w", serviceName, err)
			}
			defer func() {
				if err := resp.Body.Close(); err != nil {
					log.Error("Failed to close request body", "error", err)
				}
			}()

			if resp.StatusCode < 200 || resp.StatusCode >= 300 {
				log.Info("Failed to call /configure ", "service", serviceName, "status:", resp.Status)
				return fmt.Errorf("failed to call /configure for service %s: %s", serviceName, resp.Status)
			} else {
				log.Info("Successfully called /configure ", "for service", serviceName, "url:", url)
			}
		}

	}

	return nil
}

// GenerateComputeSpec generates a compute specification JSON response
func GenerateComputeSpec(
	ctx context.Context,
	log *slog.Logger,
	k8sClient client.Client,
	request *ComputeHookNotifyRequest,
	computeID string,
) (*ComputeSpecResponse, error) {
	log.Info("Starting compute spec generation", "compute_id", computeID)

	// 1. Find the compute deployment to get cluster context
	deployment, err := findComputeDeployment(ctx, k8sClient, computeID)
	if err != nil {
		log.Error("Failed to find compute deployment", "compute_id", computeID, "error", err)
		return nil, err
	}

	clusterName, err := extractClusterName(deployment)
	if err != nil {
		log.Error("Failed to extract cluster name from deployment", "error", err)
		return nil, err
	}

	log.Info("Found cluster name", "cluster_name", clusterName)

	tenantID, ok := deployment.GetLabels()["neon.tenant_id"]
	if !ok {
		err := fmt.Errorf("deployment missing neon.tenant_id label")
		log.Error("Missing required label", "error", err)
		return nil, err
	}

	timelineID, ok := deployment.GetLabels()["neon.timeline_id"]
	if !ok {
		err := fmt.Errorf("deployment missing neon.timeline_id label")
		log.Error("Missing required label", "error", err)
		return nil, err
	}

	log.Info("Found tenant and timeline", "tenant_id", tenantID, "timeline_id", timelineID)

	// 2. Get project and branch details
	project, branch, err := findProjectAndBranch(ctx, k8sClient, tenantID, timelineID)
	if err != nil {
		log.Error("Failed to find project and branch", "error", err,
			"tenant_id", tenantID, "timeline_id", timelineID)
		return nil, err
	}

	// 3. Get JWT keys from cluster secret
	jwks, err := getJWTKeysFromSecret(ctx, k8sClient, clusterName)
	if err != nil {
		log.Error("Failed to get JWT keys from secret", "cluster_name", clusterName, "error", err)
		return nil, err
	}

	log.Info("Successfully retrieved JWT keys")

	// 4. Construct safekeeper connections (always 3)
	safekeeperConnstrings := make([]string, 3)
	for i := range 3 {
		safekeeperConnstrings[i] = fmt.Sprintf(
			"postgresql://postgres:@%s-safekeeper-%d.neon:5454",
			clusterName, i,
		)
	}

	// 5. Build postgres settings
	settings := buildPostgresSettings(clusterName, project.Spec.TenantID, branch.Spec.TimelineID)

	// 6. Generate spec
	shards := make(map[string]PageserverShardInfo)

	var actualRequest *ComputeHookNotifyRequest
	if request != nil {
		actualRequest = request
	} else {
		// Create fallback request using storage controller client
		storageClient := NewStorageControllerClient(clusterName)
		tenantInfo, err := storageClient.GetTenantInfo(ctx, log, tenantID)
		if err != nil {
			log.Error("Failed to retrieve tenant info", "tenantID", tenantID, "error", err)
			return nil, err
		}

		log.Info("Retrieved tenant info", "tenantID", tenantID, "shards", len(tenantInfo.Shards))

		fallbackShards := make([]ComputeHookNotifyRequestShard, len(tenantInfo.Shards))
		for i, shard := range tenantInfo.Shards {
			fallbackShards[i] = ComputeHookNotifyRequestShard{
				NodeID:      shard.NodeAttached,
				ShardNumber: uint32(i),
			}
		}

		actualRequest = &ComputeHookNotifyRequest{
			TenantID:   tenantInfo.TenantID,
			StripeSize: &tenantInfo.StripeSize,
			Shards:     fallbackShards,
		}
	}

	for _, shard := range actualRequest.Shards {
		shardIndex := ShardIndex{
			ShardNumber: 0,
			ShardCount:  uint8(len(actualRequest.Shards)),
		}

		shards[shardIndex.String()] = PageserverShardInfo{
			Pageservers: []PageserverShardConnectionInfo{
				{
					ID: &shard.NodeID,
					LibpqURL: stringPtr(fmt.Sprintf(
						"postgres://no_user@%s-pageserver-%d.neon:6400",
						clusterName, shard.NodeID,
					)),
					GrpcURL: nil,
				},
			},
		}
	}

	spec := &ComputeSpecResponse{
		Spec: ComputeSpec{
			FormatVersion:         1.0,
			SuspendTimeoutSeconds: -1,
			Cluster: ClusterConfig{
				ClusterID: project.Spec.TenantID,
				Name:      project.Name,
				Roles: []Role{
					{
						Name:              "postgres",
						EncryptedPassword: "b093c0d3b281ba6da1eacc608620abd8",
						Options:           nil,
					},
				},
				Databases: []interface{}{},
				Settings:  settings,
			},
			DeltaOperations:       []interface{}{},
			SafekeeperConnstrings: safekeeperConnstrings,
			PageserverConnectionInfo: PageserverConnectionInfo{
				ShardCount: len(shards),
				Shards:     shards,
			},
		},
		ComputeCtlConfig: ComputeCtlConfig{
			JWKS: jwks,
		},
		Status: "attached",
	}

	return spec, nil
}

// Helper function to create string pointer
func stringPtr(s string) *string {
	return &s
}

// FindTenantDeployments finds deployments with the specified tenant ID
func FindTenantDeployments(ctx context.Context,
	k8sClient client.Client,
	tenantID string) (*appsv1.DeploymentList, error) {
	deploymentList := &appsv1.DeploymentList{}

	// List all deployments with the tenant_id label
	err := k8sClient.List(ctx, deploymentList, client.MatchingLabels{
		"neon.tenant_id": tenantID,
	})
	if err != nil {
		return nil, fmt.Errorf("failed to list deployments: %w", err)
	}

	if len(deploymentList.Items) == 0 {
		return nil, fmt.Errorf("no deployment available with the tenantID %s", tenantID)
	}

	return deploymentList, nil
}

// Placeholder functions that need to be implemented elsewhere
func findComputeDeployment(ctx context.Context, k8sClient client.Client, computeID string) (*appsv1.Deployment, error) {
	deploymentList := &appsv1.DeploymentList{}

	// List all deployments with the compute annotation
	err := k8sClient.List(ctx, deploymentList, client.MatchingLabels{
		"molnett.org/component": "compute",
	})
	if err != nil {
		return nil, fmt.Errorf("failed to list deployments: %w", err)
	}

	// Find deployment with matching compute_id annotation
	for _, deployment := range deploymentList.Items {
		if annotations := deployment.GetAnnotations(); annotations != nil {
			if computeIDAnnotation, exists := annotations["neon.compute_id"]; exists && computeIDAnnotation == computeID {
				return &deployment, nil
			}
		}
	}

	return nil, fmt.Errorf("deployment with compute_id %s not found", computeID)
}

func extractClusterName(deployment *appsv1.Deployment) (string, error) {
	if annotations := deployment.GetAnnotations(); annotations != nil {
		if clusterName, exists := annotations["neon.cluster_name"]; exists {
			return clusterName, nil
		}
	}

	if labels := deployment.GetLabels(); labels != nil {
		if clusterName, exists := labels["molnett.org/cluster"]; exists {
			return clusterName, nil
		}
	}

	return "", fmt.Errorf("cluster name not found in deployment metadata")
}

func findProjectAndBranch(
	ctx context.Context,
	k8sClient client.Client,
	tenantID, timelineID string,
) (*neonv1alpha1.Project, *neonv1alpha1.Branch, error) {
	// Find project by tenant ID
	projectList := &neonv1alpha1.ProjectList{}
	err := k8sClient.List(ctx, projectList)
	if err != nil {
		return nil, nil, fmt.Errorf("failed to list projects: %w", err)
	}

	var project *neonv1alpha1.Project
	for i, p := range projectList.Items {
		if p.Spec.TenantID == tenantID {
			project = &projectList.Items[i]
			break
		}
	}
	if project == nil {
		return nil, nil, fmt.Errorf("project with tenant_id %s not found", tenantID)
	}

	// Find branch by timeline ID
	branchList := &neonv1alpha1.BranchList{}
	err = k8sClient.List(ctx, branchList)
	if err != nil {
		return nil, nil, fmt.Errorf("failed to list branches: %w", err)
	}

	var branch *neonv1alpha1.Branch
	for i, b := range branchList.Items {
		if b.Spec.TimelineID == timelineID && b.Spec.ProjectID == project.Name {
			branch = &branchList.Items[i]
			break
		}
	}
	if branch == nil {
		return nil, nil, fmt.Errorf("branch with timeline_id %s not found", timelineID)
	}

	return project, branch, nil
}

func getJWTKeysFromSecret(
	ctx context.Context,
	k8sClient client.Client,
	clusterName string,
) (*utils.JWKResponse, error) {
	secretName := fmt.Sprintf("cluster-%s-jwt", clusterName)
	secret := &corev1.Secret{}

	if err := k8sClient.Get(ctx, types.NamespacedName{Name: secretName, Namespace: "neon"}, secret); err != nil {
		return nil, fmt.Errorf("failed to get JWT secret %s: %w", secretName, err)
	}

	jwtManager, err := utils.NewJWTManagerFromSecret(secret)
	if err != nil {
		return nil, fmt.Errorf("failed to create JWT manager from secret: %w", err)
	}

	return &utils.JWKResponse{
		Keys: []*utils.JWK{jwtManager.ToJWK()},
	}, nil
}

func buildPostgresSettings(clusterName, tenantID, timelineID string) []SettingsEntry {
	return []SettingsEntry{
		{Name: "fsync", Value: "off", Vartype: "bool"},
		{Name: "wal_level", Value: "logical", Vartype: "enum"},
		{Name: "wal_log_hints", Value: "on", Vartype: "bool"},
		{Name: "log_connections", Value: "on", Vartype: "bool"},
		{Name: "port", Value: "55433", Vartype: "integer"},
		{Name: "shared_buffers", Value: "1MB", Vartype: "string"},
		{Name: "max_connections", Value: "100", Vartype: "integer"},
		{Name: "listen_addresses", Value: "0.0.0.0", Vartype: "string"},
		{Name: "max_wal_senders", Value: "10", Vartype: "integer"},
		{Name: "max_replication_slots", Value: "10", Vartype: "integer"},
		{Name: "wal_sender_timeout", Value: "5s", Vartype: "string"},
		{Name: "wal_keep_size", Value: "0", Vartype: "integer"},
		{Name: "password_encryption", Value: "md5", Vartype: "enum"},
		{Name: "restart_after_crash", Value: "off", Vartype: "bool"},
		{Name: "synchronous_standby_names", Value: "walproposer", Vartype: "string"},
		{Name: "shared_preload_libraries", Value: "neon", Vartype: "string"},
		{
			Name: "neon.safekeepers",
			Value: fmt.Sprintf(
				"%s-safekeeper-0.neon:5454,%s-safekeeper-1.neon:5454,%s-safekeeper-2.neon:5454",
				clusterName, clusterName, clusterName,
			),
			Vartype: "string",
		},
		{Name: "neon.timeline_id", Value: timelineID, Vartype: "string"},
		{Name: "neon.tenant_id", Value: tenantID, Vartype: "string"},
		{Name: "neon.max_file_cache_size", Value: "1GB", Vartype: "string"},
	}
}

// StorageControllerClient placeholder
type StorageControllerClient struct {
	clusterName string
}

func NewStorageControllerClient(clusterName string) *StorageControllerClient {
	return &StorageControllerClient{clusterName: clusterName}
}

type TenantInfo struct {
	TenantID   string      `json:"tenant_id"`
	StripeSize uint32      `json:"stripe_size"`
	Shards     []ShardInfo `json:"shards"`
}

type ShardInfo struct {
	NodeAttached uint64 `json:"node_attached"`
}

func (c *StorageControllerClient) GetTenantInfo(
	ctx context.Context,
	log *slog.Logger,
	tenantID string,
) (*TenantInfo, error) {
	url := fmt.Sprintf("http://%s-storage-controller:8080/control/v1/tenant/%s", c.clusterName, tenantID)

	httpClient := &http.Client{Timeout: 10 * time.Second}
	resp, err := httpClient.Get(url)
	if err != nil {
		return nil, fmt.Errorf("failed to get tenant info: %w", err)
	}
	defer func() {
		if err := resp.Body.Close(); err != nil {
			log.Error("failed to close response body", "error", err)
		}
	}()

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("storage controller returned status %d", resp.StatusCode)
	}

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, fmt.Errorf("failed to read response body: %w", err)
	}

	log.Info("Response body", "tenantID", tenantID, "body", string(body))

	var tenantInfo TenantInfo
	if err := json.Unmarshal(body, &tenantInfo); err != nil {
		return nil, fmt.Errorf("failed to decode tenant info: %w", err)
	}

	return &tenantInfo, nil
}
