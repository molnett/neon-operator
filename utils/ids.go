package utils

import (
	"crypto/rand"
	"encoding/hex"
)

// GenerateNeonID generates a 32 character hexadecimal string suitable for use with Neon resources
func GenerateNeonID() string {
	bytes := make([]byte, 16)
	// Note: rand.Read only returns error for insufficient bytes, which won't happen with fixed-size slice
	_, _ = rand.Read(bytes)

	return hex.EncodeToString(bytes)
}
