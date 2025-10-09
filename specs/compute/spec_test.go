package compute

import (
	"encoding/json"
	"testing"
)

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
