use crate::util::errors::{Error, Result, StdError};
use actix_web::http::header::Encoding;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{Duration, Utc};
use ed25519_dalek::{pkcs8, SigningKey, VerifyingKey};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use k8s_openapi::ByteString;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use tracing::instrument::WithSubscriber;

#[derive(Debug, Serialize, Deserialize)]
pub struct JwtClaims {
    pub exp: i64, // Expiration time (required)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iat: Option<i64>, // Issued at (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub: Option<String>, // Subject (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iss: Option<String>, // Issuer (optional)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub aud: Vec<String>, // Audience (optional)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>, // Scopes (optional)
    #[serde(flatten)]
    pub custom_claims: BTreeMap<String, String>, // Custom claims (optional)
}

#[derive(Debug, Clone)]
pub struct Ed25519KeyPair {
    pub signing_key: SigningKey,
    pub verifying_key: VerifyingKey,
    pub kid: String,
}

impl Ed25519KeyPair {
    pub fn generate() -> Result<Self> {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        let kid = Self::generate_kid(&verifying_key);

        Ok(Ed25519KeyPair {
            signing_key,
            verifying_key,
            kid,
        })
    }

    pub fn generate_jwt_token(
        &self,
        expiry_duration: Duration,
        subject: Option<&str>,
        issuer: Option<&str>,
        audience: Vec<String>,
        scopes: Vec<String>,
        custom_claims: Option<BTreeMap<String, String>>,
    ) -> Result<String> {
        let now = Utc::now();
        let exp = now + expiry_duration;

        let claims = JwtClaims {
            exp: exp.timestamp(),
            iat: Some(now.timestamp()),
            sub: subject.map(|s| s.to_string()),
            iss: issuer.map(|s| s.to_string()),
            aud: audience,
            scopes,
            custom_claims: custom_claims.unwrap_or_default(),
        };

        let mut header = Header::new(Algorithm::EdDSA);
        header.kid = Some(self.kid.clone());

        let private_key = pkcs8::EncodePrivateKey::to_pkcs8_der(&self.signing_key).unwrap();

        encode(
            &header,
            &claims,
            &EncodingKey::from_ed_der(private_key.as_bytes()),
        )
        .map_err(|e| Error::StdError(StdError::CryptoError(e.to_string())))
    }

    fn generate_kid(verifying_key: &VerifyingKey) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(verifying_key.as_bytes());
        let hash = hasher.finalize();
        URL_SAFE_NO_PAD.encode(&hash)
    }

    pub fn to_jwk(&self) -> Value {
        json!({
            "use": "sig",
            "key_ops": ["verify"],
            "alg": "EdDSA",
            "kid": self.kid,
            "kty": "OKP",
            "crv": "Ed25519",
            "x": URL_SAFE_NO_PAD.encode(self.verifying_key.as_bytes())
        })
    }

    pub fn to_jwks(&self) -> Value {
        json!({
            "keys": [self.to_jwk()]
        })
    }

    pub fn to_secret_data(&self) -> Result<BTreeMap<String, ByteString>> {
        let mut data = BTreeMap::new();

        data.insert(
            "verifying_key".to_string(),
            ByteString(
                URL_SAFE_NO_PAD
                    .encode(self.verifying_key.as_bytes())
                    .as_bytes()
                    .to_vec(),
            ),
        );

        data.insert(
            "signing_key".to_string(),
            ByteString(
                URL_SAFE_NO_PAD
                    .encode(self.signing_key.to_bytes())
                    .as_bytes()
                    .to_vec(),
            ),
        );

        data.insert("kid".to_string(), ByteString(self.kid.as_bytes().to_vec()));

        data.insert(
            "jwk".to_string(),
            ByteString(
                serde_json::to_string(&self.to_jwk())
                    .map_err(|e| Error::StdError(StdError::JsonSerializationError(e)))?
                    .into_bytes()
                    .to_vec(),
            ),
        );

        data.insert(
            "jwks".to_string(),
            ByteString(
                serde_json::to_string(&self.to_jwks())
                    .map_err(|e| Error::StdError(StdError::JsonSerializationError(e)))?
                    .into_bytes()
                    .to_vec(),
            ),
        );

        Ok(data)
    }

    pub fn from_secret_data(data: &BTreeMap<String, ByteString>) -> Result<Self> {
        let verifying_key_b64 = String::from_utf8(
            data.get("verifying_key")
                .ok_or_else(|| {
                    Error::StdError(StdError::MetadataMissing(
                        "verifying_key not found in secret".into(),
                    ))
                })?
                .clone()
                .0,
        )
        .map_err(|_| Error::StdError(StdError::MetadataMissing("Invalid UTF-8 in verifying_key".into())))?;

        let signing_key_b64 = String::from_utf8(
            data.get("signing_key")
                .ok_or_else(|| {
                    Error::StdError(StdError::MetadataMissing(
                        "signing_key not found in secret".into(),
                    ))
                })?
                .clone()
                .0,
        )
        .map_err(|_| Error::StdError(StdError::MetadataMissing("Invalid UTF-8 in signing_key".into())))?;

        let kid = String::from_utf8(
            data.get("kid")
                .ok_or_else(|| Error::StdError(StdError::MetadataMissing("kid not found in secret".into())))?
                .clone()
                .0,
        )
        .map_err(|_| Error::StdError(StdError::MetadataMissing("Invalid UTF-8 in kid".into())))?;

        let verifying_key_bytes = URL_SAFE_NO_PAD
            .decode(&verifying_key_b64)
            .map_err(|e| Error::StdError(StdError::DecodingError(e.to_string())))?;

        let signing_key_bytes = URL_SAFE_NO_PAD
            .decode(&signing_key_b64)
            .map_err(|e| Error::StdError(StdError::DecodingError(e.to_string())))?;

        let verifying_key = VerifyingKey::from_bytes(&verifying_key_bytes.try_into().map_err(|_| {
            Error::StdError(StdError::CryptoError("Invalid verifying key length".to_string()))
        })?)
        .map_err(|e| Error::StdError(StdError::CryptoError(e.to_string())))?;

        let signing_key =
            SigningKey::from_bytes(&signing_key_bytes.try_into().map_err(|_| {
                Error::StdError(StdError::CryptoError("Invalid signing key length".to_string()))
            })?);

        Ok(Ed25519KeyPair {
            signing_key,
            verifying_key,
            kid,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_generation() {
        let keypair = Ed25519KeyPair::generate().unwrap();
        assert!(!keypair.kid.is_empty());

        // Test JWK format
        let jwk = keypair.to_jwk();
        assert_eq!(jwk["use"], "sig");
        assert_eq!(jwk["alg"], "EdDSA");
        assert_eq!(jwk["kty"], "OKP");
        assert_eq!(jwk["crv"], "Ed25519");
        assert_eq!(jwk["kid"], keypair.kid);
    }

    #[test]
    fn test_secret_roundtrip() {
        let keypair = Ed25519KeyPair::generate().unwrap();
        let secret_data = keypair.to_secret_data().unwrap();
        let recovered = Ed25519KeyPair::from_secret_data(&secret_data).unwrap();

        assert_eq!(keypair.kid, recovered.kid);
        assert_eq!(
            keypair.verifying_key.as_bytes(),
            recovered.verifying_key.as_bytes()
        );
        assert_eq!(keypair.signing_key.to_bytes(), recovered.signing_key.to_bytes());
    }

    #[test]
    fn test_jwt_token_generation() {
        let keypair = Ed25519KeyPair::generate().unwrap();
        let scopes = vec!["pageserverapi".to_string(), "safekeeper".to_string()];
        let expiry = Duration::hours(1);

        let token = keypair
            .generate_jwt_token(
                expiry,
                Some("test-user"),
                Some("neon-operator"),
                vec!["neon-cluster".to_string()],
                scopes.clone(),
                None,
            )
            .unwrap();

        // Token should be non-empty and have JWT format (3 parts separated by dots)
        assert!(!token.is_empty());
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);

        // Decode and verify the header contains our kid
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
        let header_json = URL_SAFE_NO_PAD.decode(&parts[0]).unwrap();
        let header: Value = serde_json::from_slice(&header_json).unwrap();
        assert_eq!(header["kid"], keypair.kid);
        assert_eq!(header["alg"], "EdDSA");

        // Decode and verify the claims
        let claims_json = URL_SAFE_NO_PAD.decode(&parts[1]).unwrap();
        let claims: JwtClaims = serde_json::from_slice(&claims_json).unwrap();
        assert_eq!(claims.sub, Some("test-user".to_string()));
        assert_eq!(claims.iss, Some("neon-operator".to_string()));
        assert_eq!(claims.aud[0], "neon-cluster".to_string());
        assert_eq!(claims.scopes, scopes);
        assert!(claims.exp > claims.iat.unwrap());
    }

    #[test]
    fn test_jwt_token_with_custom_claims() {
        let keypair = Ed25519KeyPair::generate().unwrap();
        let expiry = Duration::hours(1);

        let mut custom_claims = BTreeMap::new();
        custom_claims.insert("tenant_id".to_string(), "tenant-123".to_string());
        custom_claims.insert("branch_id".to_string(), "branch-456".to_string());
        custom_claims.insert("compute_id".to_string(), "compute-789".to_string());

        let token = keypair
            .generate_jwt_token(
                expiry,
                Some("test-user"),
                Some("neon-operator"),
                vec!["neon-cluster".to_string()],
                vec!["pageserverapi".to_string()],
                Some(custom_claims.clone()),
            )
            .unwrap();

        // Decode and verify the claims contain custom fields
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
        let parts: Vec<&str> = token.split('.').collect();
        let claims_json = URL_SAFE_NO_PAD.decode(&parts[1]).unwrap();
        let claims: JwtClaims = serde_json::from_slice(&claims_json).unwrap();

        assert_eq!(
            claims.custom_claims.get("tenant_id"),
            Some(&"tenant-123".to_string())
        );
        assert_eq!(
            claims.custom_claims.get("branch_id"),
            Some(&"branch-456".to_string())
        );
        assert_eq!(
            claims.custom_claims.get("compute_id"),
            Some(&"compute-789".to_string())
        );
    }
}
