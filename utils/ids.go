package utils

import (
	"math/rand"
	"time"
)

var charset = []rune("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890")

// GenerateNeonID generates a 32 character alphanumeric string suitable for use with Neon resources
func GenerateNeonID() string {
	var seededRand = rand.New(rand.NewSource(time.Now().UnixNano()))
	b := make([]rune, 32)
	for i := range b {
		b[i] = charset[seededRand.Intn(len(charset))]
	}
	return string(b)
}
