package utils

import (
	"crypto/ed25519"
	"crypto/rand"
	"crypto/x509"
	"encoding/base64"
	"encoding/json"
	"encoding/pem"
	"testing"
	"time"

	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
)

func TestNewJWTManagerFromSecret(t *testing.T) {
	// Generate test keys
	pubKey, privKey, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("failed to generate test keys: %v", err)
	}

	// Marshal keys to PEM format
	privKeyBytes, err := x509.MarshalPKCS8PrivateKey(privKey)
	if err != nil {
		t.Fatalf("failed to marshal private key: %v", err)
	}

	privKeyPEM := pem.EncodeToMemory(&pem.Block{
		Type:  "PRIVATE KEY",
		Bytes: privKeyBytes,
	})

	pubKeyBytes, err := x509.MarshalPKIXPublicKey(pubKey)
	if err != nil {
		t.Fatalf("failed to marshal public key: %v", err)
	}

	pubKeyPEM := pem.EncodeToMemory(&pem.Block{
		Type:  "PUBLIC KEY",
		Bytes: pubKeyBytes,
	})

	tests := []struct {
		name        string
		secret      *corev1.Secret
		expectError bool
	}{
		{
			name: "valid secret",
			secret: &corev1.Secret{
				ObjectMeta: metav1.ObjectMeta{
					Name:      "test-secret",
					Namespace: "default",
				},
				Data: map[string][]byte{
					"private.pem": privKeyPEM,
					"public.pem":  pubKeyPEM,
				},
			},
			expectError: false,
		},
		{
			name: "missing private key",
			secret: &corev1.Secret{
				ObjectMeta: metav1.ObjectMeta{
					Name:      "test-secret",
					Namespace: "default",
				},
				Data: map[string][]byte{
					"public.pem": pubKeyPEM,
				},
			},
			expectError: true,
		},
		{
			name: "missing public key",
			secret: &corev1.Secret{
				ObjectMeta: metav1.ObjectMeta{
					Name:      "test-secret",
					Namespace: "default",
				},
				Data: map[string][]byte{
					"private.pem": privKeyPEM,
				},
			},
			expectError: true,
		},
		{
			name: "invalid private key PEM",
			secret: &corev1.Secret{
				ObjectMeta: metav1.ObjectMeta{
					Name:      "test-secret",
					Namespace: "default",
				},
				Data: map[string][]byte{
					"private.pem": []byte("invalid-pem"),
					"public.pem":  pubKeyPEM,
				},
			},
			expectError: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			jm, err := NewJWTManagerFromSecret(tt.secret)

			if tt.expectError {
				if err == nil {
					t.Errorf("expected error but got none")
				}
				return
			}

			if err != nil {
				t.Errorf("unexpected error: %v", err)
				return
			}

			if jm == nil {
				t.Errorf("expected JWT manager but got nil")
			}
		})
	}
}

func TestJWTManager_GenerateAndVerifyToken(t *testing.T) {
	// Generate test keys
	pubKey, privKey, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("failed to generate test keys: %v", err)
	}

	jm := &JWTManager{
		privateKey: privKey,
		publicKey:  pubKey,
	}

	now := time.Now()
	claims := map[string]interface{}{
		"sub":    "test-subject",
		"iss":    "neon-operator",
		"exp":    now.Add(time.Hour).Unix(), // expires in 1 hour
		"iat":    now.Unix(),                // issued now
		"aud":    "test-audience",
		"custom": "test-value",
	}

	// Generate token
	tokenString, err := jm.GenerateToken(claims)
	if err != nil {
		t.Fatalf("failed to generate token: %v", err)
	}

	if tokenString == "" {
		t.Errorf("expected token string but got empty string")
	}

	// Verify token
	token, err := jm.VerifyToken(tokenString)
	if err != nil {
		t.Fatalf("failed to verify token: %v", err)
	}

	// Check standard claims
	if sub, ok := token.Subject(); !ok || sub != "test-subject" {
		t.Errorf("expected sub claim to be 'test-subject', got %s (ok: %t)", sub, ok)
	}

	if iss, ok := token.Issuer(); !ok || iss != "neon-operator" {
		t.Errorf("expected iss claim to be 'neon-operator', got %s (ok: %t)", iss, ok)
	}

	// Check if token parsing succeeded (basic verification that token is valid)
	if token == nil {
		t.Errorf("expected valid token but got nil")
	}
}

func TestJWTManager_VerifyToken_InvalidSignature(t *testing.T) {
	// Generate two different key pairs
	pubKey1, privKey1, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("failed to generate first key pair: %v", err)
	}

	pubKey2, privKey2, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("failed to generate second key pair: %v", err)
	}

	// Create manager with first key pair
	jm1 := &JWTManager{
		privateKey: privKey1,
		publicKey:  pubKey1,
	}

	// Create manager with second key pair
	jm2 := &JWTManager{
		privateKey: privKey2,
		publicKey:  pubKey2,
	}

	claims := map[string]interface{}{
		"sub": "test-subject",
	}

	// Generate token with first manager
	tokenString, err := jm1.GenerateToken(claims)
	if err != nil {
		t.Fatalf("failed to generate token: %v", err)
	}

	// Try to verify with second manager (should fail)
	_, err = jm2.VerifyToken(tokenString)
	if err == nil {
		t.Errorf("expected verification to fail with different key pair, but it succeeded")
	}
}

func TestJWTManager_ToJWK(t *testing.T) {
	// Generate test keys
	pubKey, privKey, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("failed to generate test keys: %v", err)
	}

	jm := &JWTManager{
		privateKey: privKey,
		publicKey:  pubKey,
	}

	jwk := jm.ToJWK()

	// Verify JWK structure
	if jwk.Use != "sig" {
		t.Errorf("expected use to be 'sig', got %s", jwk.Use)
	}

	if len(jwk.KeyOps) != 1 || jwk.KeyOps[0] != "verify" {
		t.Errorf("expected key_ops to be ['verify'], got %v", jwk.KeyOps)
	}

	if jwk.Alg != "EdDSA" {
		t.Errorf("expected alg to be 'EdDSA', got %s", jwk.Alg)
	}

	if jwk.Kid != "neon-operator" {
		t.Errorf("expected kid to be 'neon-operator', got %s", jwk.Kid)
	}

	if jwk.Kty != "OKP" {
		t.Errorf("expected kty to be 'OKP', got %s", jwk.Kty)
	}

	if jwk.Crv != "Ed25519" {
		t.Errorf("expected crv to be 'Ed25519', got %s", jwk.Crv)
	}

	// Verify X parameter (base64url encoded public key)
	expectedX := base64.RawURLEncoding.EncodeToString(pubKey)
	if jwk.X != expectedX {
		t.Errorf("expected x to be %s, got %s", expectedX, jwk.X)
	}

	// Test JSON marshaling
	jsonBytes, err := json.Marshal(jwk)
	if err != nil {
		t.Fatalf("failed to marshal JWK to JSON: %v", err)
	}

	var unmarshaledJWK JWKResponse
	if err := json.Unmarshal(jsonBytes, &unmarshaledJWK); err != nil {
		t.Fatalf("failed to unmarshal JWK from JSON: %v", err)
	}

	if unmarshaledJWK.Keys[0].Use != jwk.Use {
		t.Errorf("JSON marshaling/unmarshaling changed use field")
	}
	if unmarshaledJWK.Keys[0].X != jwk.X {
		t.Errorf("JSON marshaling/unmarshaling changed x field")
	}
}

func TestJWTManager_GenerateToken_EmptyClaims(t *testing.T) {
	// Generate test keys
	pubKey, privKey, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("failed to generate test keys: %v", err)
	}

	jm := &JWTManager{
		privateKey: privKey,
		publicKey:  pubKey,
	}

	// Generate token with empty claims
	claims := map[string]interface{}{}
	tokenString, err := jm.GenerateToken(claims)
	if err != nil {
		t.Fatalf("failed to generate token with empty claims: %v", err)
	}

	if tokenString == "" {
		t.Errorf("expected token string but got empty string")
	}

	// Verify the token can be parsed
	token, err := jm.VerifyToken(tokenString)
	if err != nil {
		t.Fatalf("failed to verify token with empty claims: %v", err)
	}

	if token == nil {
		t.Errorf("expected valid token but got nil")
	}
}

func TestJWTManager_VerifyToken_InvalidToken(t *testing.T) {
	// Generate test keys
	pubKey, privKey, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("failed to generate test keys: %v", err)
	}

	jm := &JWTManager{
		privateKey: privKey,
		publicKey:  pubKey,
	}

	// Test with invalid token strings
	invalidTokens := []string{
		"",
		"invalid-token",
		"header.payload.signature",
		"not.a.jwt",
	}

	for _, invalidToken := range invalidTokens {
		t.Run("invalid_token_"+invalidToken, func(t *testing.T) {
			_, err := jm.VerifyToken(invalidToken)
			if err == nil {
				t.Errorf("expected error when verifying invalid token %q, but got none", invalidToken)
			}
		})
	}
}
