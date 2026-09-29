use crate::api;
use anyhow::{Result, anyhow};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use uuid::Uuid;

pub fn extract_claims_from_jwt(token: &str) -> Result<api::UserClaims> {
    let segments: Vec<&str> = token.split('.').collect();
    if segments.len() != 3 {
        return Err(anyhow!("Malformed JWT token format string."));
    }

    // Decode the middle segment (Index 1) using standard URL-Safe Base64
    let decoded_bytes = URL_SAFE_NO_PAD
        .decode(segments[1])
        .map_err(|e| anyhow!("Failed to decode JWT Base64 segment: {}", e))?;

    let raw_payload: api::JwtPayload = serde_json::from_slice(&decoded_bytes)
        .map_err(|e| anyhow!("Failed to map JWT fields: {}", e))?;

    let user_uuid = Uuid::parse_str(&raw_payload.id).unwrap_or_else(|_| Uuid::new_v4());

    Ok(api::UserClaims {
        id: user_uuid,
        username: raw_payload.username.into(),
        email: None,
    })
}
