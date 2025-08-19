package utils

import (
	"crypto/rand"
	"encoding/hex"
)

// GenerateNeonID generates a 32 character hexadecimal string suitable for use with Neon resources
func GenerateNeonID() string {
	bytes := make([]byte, 16)
	rand.Read(bytes)

	return hex.EncodeToString(bytes)
}
