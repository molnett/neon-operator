use crate::util::errors::{Error, Result, StdError};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use serde_json::{json, Value};
use std::collections::BTreeMap;

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

    pub fn to_secret_data(&self) -> Result<BTreeMap<String, Vec<u8>>> {
        let mut data = BTreeMap::new();
        
        data.insert(
            "verifying_key".to_string(),
            URL_SAFE_NO_PAD.encode(self.verifying_key.as_bytes()).into_bytes(),
        );
        
        data.insert(
            "signing_key".to_string(),
            URL_SAFE_NO_PAD.encode(self.signing_key.to_bytes()).into_bytes(),
        );
        
        data.insert(
            "kid".to_string(),
            self.kid.as_bytes().to_vec(),
        );
        
        data.insert(
            "jwk".to_string(),
            serde_json::to_string(&self.to_jwk())
                .map_err(|e| Error::StdError(StdError::JsonSerializationError(e)))?
                .into_bytes(),
        );
        
        data.insert(
            "jwks".to_string(),
            serde_json::to_string(&self.to_jwks())
                .map_err(|e| Error::StdError(StdError::JsonSerializationError(e)))?
                .into_bytes(),
        );
        
        Ok(data)
    }

    pub fn from_secret_data(data: &BTreeMap<String, Vec<u8>>) -> Result<Self> {
        let verifying_key_b64 = String::from_utf8(
            data.get("verifying_key")
                .ok_or_else(|| Error::StdError(StdError::MetadataMissing("verifying_key not found in secret".into())))?
                .clone(),
        )
        .map_err(|_| Error::StdError(StdError::MetadataMissing("Invalid UTF-8 in verifying_key".into())))?;

        let signing_key_b64 = String::from_utf8(
            data.get("signing_key")
                .ok_or_else(|| Error::StdError(StdError::MetadataMissing("signing_key not found in secret".into())))?
                .clone(),
        )
        .map_err(|_| Error::StdError(StdError::MetadataMissing("Invalid UTF-8 in signing_key".into())))?;

        let kid = String::from_utf8(
            data.get("kid")
                .ok_or_else(|| Error::StdError(StdError::MetadataMissing("kid not found in secret".into())))?
                .clone(),
        )
        .map_err(|_| Error::StdError(StdError::MetadataMissing("Invalid UTF-8 in kid".into())))?;

        let verifying_key_bytes = URL_SAFE_NO_PAD
            .decode(&verifying_key_b64)
            .map_err(|e| Error::StdError(StdError::DecodingError(e.to_string())))?;

        let signing_key_bytes = URL_SAFE_NO_PAD
            .decode(&signing_key_b64)
            .map_err(|e| Error::StdError(StdError::DecodingError(e.to_string())))?;

        let verifying_key = VerifyingKey::from_bytes(
            &verifying_key_bytes.try_into()
                .map_err(|_| Error::StdError(StdError::CryptoError("Invalid verifying key length".to_string())))?
        )
            .map_err(|e| Error::StdError(StdError::CryptoError(e.to_string())))?;

        let signing_key = SigningKey::from_bytes(
            &signing_key_bytes.try_into()
                .map_err(|_| Error::StdError(StdError::CryptoError("Invalid signing key length".to_string())))?
        );

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
        assert_eq!(keypair.verifying_key.as_bytes(), recovered.verifying_key.as_bytes());
        assert_eq!(keypair.signing_key.to_bytes(), recovered.signing_key.to_bytes());
    }
}