use crate::api;
use crate::debug_log;
use anyhow::{Result, anyhow};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::Client;
use std::sync::Arc;
use uuid::Uuid;
use crate::api::API_URLS;
// Pulling definitions from section above




use crate::auth::jwt::extract_claims_from_jwt;
