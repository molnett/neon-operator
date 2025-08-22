package utils

import (
	"crypto/ed25519"
	"crypto/x509"
	"encoding/base64"
	"encoding/pem"
	"fmt"

	"github.com/lestrrat-go/jwx/v3/jwa"
	"github.com/lestrrat-go/jwx/v3/jwt"
	corev1 "k8s.io/api/core/v1"
)

// JWTManager handles JWT operations using keys from Kubernetes secrets
type JWTManager struct {
	privateKey ed25519.PrivateKey
	publicKey  ed25519.PublicKey
}

// NewJWTManagerFromSecret creates a JWTManager from a Kubernetes secret
func NewJWTManagerFromSecret(secret *corev1.Secret) (*JWTManager, error) {
	privKeyPEM, ok := secret.Data["private.pem"]
	if !ok {
		return nil, fmt.Errorf("private.pem not found in secret")
	}

	pubKeyPEM, ok := secret.Data["public.pem"]
	if !ok {
		return nil, fmt.Errorf("public.pem not found in secret")
	}

	// Parse private key
	privBlock, _ := pem.Decode(privKeyPEM)
	if privBlock == nil {
		return nil, fmt.Errorf("failed to decode private key PEM")
	}

	privKey, err := x509.ParsePKCS8PrivateKey(privBlock.Bytes)
	if err != nil {
		return nil, fmt.Errorf("failed to parse private key: %w", err)
	}

	ed25519PrivKey, ok := privKey.(ed25519.PrivateKey)
	if !ok {
		return nil, fmt.Errorf("private key is not Ed25519")
	}

	// Parse public key
	pubBlock, _ := pem.Decode(pubKeyPEM)
	if pubBlock == nil {
		return nil, fmt.Errorf("failed to decode public key PEM")
	}

	pubKey, err := x509.ParsePKIXPublicKey(pubBlock.Bytes)
	if err != nil {
		return nil, fmt.Errorf("failed to parse public key: %w", err)
	}

	ed25519PubKey, ok := pubKey.(ed25519.PublicKey)
	if !ok {
		return nil, fmt.Errorf("public key is not Ed25519")
	}

	return &JWTManager{
		privateKey: ed25519PrivKey,
		publicKey:  ed25519PubKey,
	}, nil
}

// GenerateToken creates a new JWT token with the given claims
func (jm *JWTManager) GenerateToken(claims map[string]any) (string, error) {
	token := jwt.New()

	// Add claims to the token
	for key, value := range claims {
		if err := token.Set(key, value); err != nil {
			return "", fmt.Errorf("failed to set claim %s: %w", key, err)
		}
	}

	// Sign the token with Ed25519 private key
	signed, err := jwt.Sign(token, jwt.WithKey(jwa.EdDSA(), jm.privateKey))
	if err != nil {
		return "", fmt.Errorf("failed to sign token: %w", err)
	}

	return string(signed), nil
}

// VerifyToken verifies and parses a JWT token
func (jm *JWTManager) VerifyToken(tokenString string) (jwt.Token, error) {
	return jwt.Parse([]byte(tokenString), jwt.WithKey(jwa.EdDSA(), jm.publicKey))
}

type JWKResponse struct {
	Keys []*JWK `json:"keys"`
}

// JWK represents the JSON Web Key structure
type JWK struct {
	Use    string   `json:"use"`
	KeyOps []string `json:"key_ops"`
	Alg    string   `json:"alg"`
	Kid    string   `json:"kid"`
	Kty    string   `json:"kty"`
	Crv    string   `json:"crv"`
	X      string   `json:"x"`
}

func (jm *JWTManager) ToJWK() *JWK {
	// Encode the public key bytes using base64url (no padding)
	x := base64.RawURLEncoding.EncodeToString(jm.publicKey)

	return &JWK{
		Use:    "sig",
		KeyOps: []string{"verify"},
		Alg:    "EdDSA",
		Kid:    "neon-operator",
		Kty:    "OKP",
		Crv:    "Ed25519",
		X:      x,
	}
}
