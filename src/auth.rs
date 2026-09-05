pub mod discord;

use reqwest::{Client};
use std::sync::Arc;
use anyhow::{anyhow, Result};
use crate::api;
use crate::debug_log;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
// Pulling definitions from section above

#[derive(Clone)]
pub struct AuthService {
    client: Client,
    auth_host: Arc<str>,
}

impl AuthService {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            auth_host: "https://idk.neurokaraoke.com".into(),
        }
    }

    /*
    /// Handles standard API errors which arrive as plain text or bare JSON strings.
    async fn handle_error_response(response: reqwest::Response) -> anyhow::Error {
        let status = response.status();
        if let Ok(body_text) = response.text().await {
            // Attempting to trim bare string serialization markers if present
            let clean_msg = body_text.trim_matches('"');
            anyhow!("Auth Error ({}): {}", status, clean_msg)
        } else {
            anyhow!("Auth request failed with network status: {}", status)
        }
    }
     */

    /*
    /// POST /api/auth/login
    pub async fn login(&self, req: &api::LoginRequest) -> Result<api::AuthContext> {
        let url = format!("{}/api/auth/login", self.auth_host);
        let res = self.client.post(&url).json(req).send().await?;

        if res.status().is_success() {
            let data: api::AuthResponse = res.json().await?;

            // ─── EXTRACT CLAIMS FROM THE RETURNED TOKEN STRING ───
            let user_claims = extract_claims_from_jwt(&data.token)?;

            Ok(api::AuthContext {
                token: data.token,
                user: user_claims
            })
        } else {
            Err(Self::handle_error_response(res).await)
        }
    }
    */

    /// POST /api/auth/discord-token
    pub async fn login_via_discord(&self, access_token: &str) -> Result<api::AuthContext> {
        let url = format!("{}/api/auth/discord-token", self.auth_host);
        let payload = api::DiscordTokenRequest { access_token: access_token.into() };
        if let Ok(json_string) = serde_json::to_string(&payload) {
            debug_log !("🔍 DEBUG OUTBOUND JSON PAYLOAD: {}", json_string);
        } else {
            debug_log !("🔍 DEBUG OUTBOUND JSON PAYLOAD: [Failed to serialize struct]");
        }
        let res = self.client.post(&url).json(&payload).send().await?;
        let status = res.status().clone();

        // ─── ADD RESPONSE TEXT CAPTURING TO PREVENT COLD DECODING CRASHES ───
        let response_text = res.text().await.unwrap_or_else(|_| "[Failed to read response body]".to_string());
        debug_log!("📥 DEBUG INBOUND RAW RESPONSE (Status {}): {}", status, response_text);


        if status.is_success() {
            // 1. Parse the flat token container
            let data: api::AuthResponse = serde_json::from_str(&response_text)?;

            // 2. Decode the inner user claims from the token string
            let user_claims = extract_claims_from_jwt(&data.token)?;

            crate::debug_log!("Successfully parsed claims for user: {}", user_claims.username);

            Ok(api::AuthContext {
                token: data.token,
                user: user_claims
            })
        } else {
            Err(anyhow!("Auth Error ({}): {}", status, response_text))
        }
    }

    /*
    /// GET /api/auth/me
    /// Verifies an active JWT structure against the core validation gate.
    pub async fn verify_token(&self, token: &str) -> Result<api::UserClaims> {
        let url = format!("{}/api/auth/me", self.auth_host);
        let res = self.client.get(&url)
            .bearer_auth(token)
            .send()
            .await?;

        if res.status().is_success() {
            Ok(res.json().await?)
        } else {
            Err(Self::handle_error_response(res).await)
        }
    }
     */


    /*
    /// POST /api/auth/qr-session
    /// Allocates an unlinked identity synchronization state for hardware logins (e.g., TV/Car clients).
    pub async fn initialize_qr_session(&self) -> Result<api::QrSession> {
        let url = format!("{}/api/auth/qr-session", self.auth_host);
        let res = self.client.post(&url).send().await?;

        if res.status().is_success() {
            Ok(res.json().await?)
        } else {
            Err(Self::handle_error_response(res).await)
        }
    }
     */

    /*
    /// GET /api/auth/qr-session/{sessionId}
    /// Polls the allocation loop to detect if a mobile/desktop controller has signed the session.
    pub async fn poll_qr_session(&self, session_id: Uuid) -> Result<Option<api::AuthContext>> {
        let url = format!("{}/api/auth/qr-session/{}", self.auth_host, session_id);
        let res = self.client.get(&url).send().await?;

        if res.status().is_success() {
            let session: api::QrSession = res.json().await?;
            if session.is_linked && let Some(token) = session.token {
                // Instantly sync context records once validation wraps up
                let user = self.verify_token(&token).await?;
                return Ok(Some(api::AuthContext { token, user }));
            }
            Ok(None)
        } else {
            Err(Self::handle_error_response(res).await)
        }
    }
     */

    /*
    /// POST /api/auth/pairing-code
    /// Generates a human-readable linking identifier to connect target hardware platforms.
    pub async fn generate_pairing_code(&self, token: &str) -> Result<String> {
        let url = format!("{}/api/auth/pairing-code", self.auth_host);
        let res = self.client.post(&url)
            .bearer_auth(token)
            .send()
            .await?;

        if res.status().is_success() {
            // Pairing codes are returned as plain string formats
            Ok(res.text().await?.trim_matches('"').to_string())
        } else {
            Err(Self::handle_error_response(res).await)
        }
    }
     */

    /*
    pub async fn get_user_profile(&self, token: &str) -> anyhow::Result<crate::api::ProfileResponse> {
        let url = "https://api.neurokaraoke.com/api/badge/profile";
        let res = self.client.get(url)
            .bearer_auth(token)
            .send()
            .await?;

        if res.status().is_success() {
            Ok(res.json().await?)
        } else {
            Err(Self::handle_error_response(res).await)
        }
    }

     */
}

fn extract_claims_from_jwt(token: &str) -> anyhow::Result<api::UserClaims> {
    let segments: Vec<&str> = token.split('.').collect();
    if segments.len() != 3 {
        return Err(anyhow::anyhow!("Malformed JWT token format string."));
    }

    // Decode the middle segment (Index 1) using standard URL-Safe Base64
    let decoded_bytes = URL_SAFE_NO_PAD.decode(segments[1])
        .map_err(|e| anyhow::anyhow!("Failed to decode JWT Base64 segment: {}", e))?;

    // ─── UPDATE THIS TYPE HOOK TO PATH REF ───
    let raw_payload: crate::api::JwtPayload = serde_json::from_slice(&decoded_bytes)
        .map_err(|e| anyhow::anyhow!("Failed to map JWT fields: {}", e))?;

    // Attempt to parse out the identity string. If the server passes a non-standard numeric string,
    // or string literal, we handle fallback mappings or map it safely to a Uuid:
    let user_uuid = uuid::Uuid::parse_str(&raw_payload.id)
        .unwrap_or_else(|_| {
            // Fallback generation for numeric/non-standard string IDs
            uuid::Uuid::new_v4()
        });

    Ok(api::UserClaims {
        id: user_uuid,
        username: raw_payload.username.into(),
        email: None,
    })
}