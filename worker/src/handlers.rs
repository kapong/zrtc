use serde::Deserialize;
use serde_json::json;
use worker::*;

use crate::crypto;
use crate::storage::{self, ChannelMeta};
use crate::vacuum;

// ── Request / Response types ────────────────────────────

#[derive(Deserialize, Default)]
pub struct NewChannelRequest {
    pub token_length: Option<usize>,
    pub passcode_length: Option<usize>,
}

#[derive(Deserialize)]
pub struct ChannelRequest {
    pub channel_id: String,
    pub passcode: String,
    #[serde(default)]
    pub role: String,
    pub signal: Option<serde_json::Value>,
}

// ── Helpers ─────────────────────────────────────────────

fn env_var_u64(env: &Env, key: &str, default: u64) -> u64 {
    env.var(key)
        .ok()
        .and_then(|v| v.to_string().parse().ok())
        .unwrap_or(default)
}

fn env_var_usize(env: &Env, key: &str, default: usize) -> usize {
    env.var(key)
        .ok()
        .and_then(|v| v.to_string().parse().ok())
        .unwrap_or(default)
}

fn now_ms() -> u64 {
    js_sys::Date::now() as u64
}

fn allowed_origin(env: &Env) -> String {
    env.var("ALLOWED_ORIGIN")
        .ok()
        .map(|v| v.to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "*".to_string())
}

fn cors_headers(origin: &str) -> Headers {
    let h = Headers::new();
    h.set("Access-Control-Allow-Origin", origin).ok();
    h.set("Content-Type", "application/json").ok();
    h
}

fn json_response(status: u16, body: serde_json::Value, env: &Env) -> Result<Response> {
    let origin = allowed_origin(env);
    Ok(Response::builder()
        .with_status(status)
        .with_headers(cors_headers(&origin))
        .body(worker::ResponseBody::Body(serde_json::to_vec(&body).unwrap()))
    )
}

fn ok_json(body: serde_json::Value, env: &Env) -> Result<Response> {
    json_response(200, body, env)
}

fn err_json(status: u16, error: &str, message: &str, env: &Env) -> Result<Response> {
    json_response(status, json!({ "error": error, "message": message }), env)
}

/// Verify passcode, incrementing attempts on failure.
async fn verify_passcode(
    bucket: &Bucket,
    channel_id: &str,
    meta: &mut ChannelMeta,
    passcode: &str,
) -> Result<bool> {
    if meta.passcode_attempts >= meta.max_passcode_attempts {
        return Ok(false);
    }
    let salt = hex::decode(&meta.salt_hex).unwrap_or_default();
    if crypto::verify_passcode(passcode, &salt, &meta.passcode_hash) {
        return Ok(true);
    }
    meta.passcode_attempts += 1;
    storage::write_meta(bucket, channel_id, meta).await?;
    Ok(false)
}

// ── Route Handlers ──────────────────────────────────────

/// POST /new or /new/:token
pub async fn handle_new(
    mut req: Request,
    env: &Env,
    _ctx: &Context,
    custom_token: Option<String>,
) -> Result<Response> {
    let bucket = storage::get_bucket(env)?;

    // Parse optional body
    let body: NewChannelRequest = req.json().await.unwrap_or_default();

    // Validate lengths
    let token_min = env_var_usize(env, "TOKEN_LENGTH_MIN", 6);
    let token_max = env_var_usize(env, "TOKEN_LENGTH_MAX", 32);
    let pass_min = env_var_usize(env, "PASSCODE_LENGTH_MIN", 4);
    let pass_max = env_var_usize(env, "PASSCODE_LENGTH_MAX", 8);
    let token_default = env_var_usize(env, "TOKEN_LENGTH_DEFAULT", 8);
    let pass_default = env_var_usize(env, "PASSCODE_LENGTH_DEFAULT", 6);

    let token_len = body.token_length.unwrap_or(token_default);
    let pass_len = body.passcode_length.unwrap_or(pass_default);

    if token_len < token_min || token_len > token_max {
        return err_json(400, "invalid_token_length", &format!("token_length must be {}-{}", token_min, token_max), env);
    }
    if pass_len < pass_min || pass_len > pass_max {
        return err_json(400, "invalid_passcode_length", &format!("passcode_length must be {}-{}", pass_min, pass_max), env);
    }

    // Generate or validate custom token
    let channel_id = match custom_token {
        Some(t) => {
            if t.len() < token_min || t.len() > token_max {
                return err_json(400, "invalid_token_length", &format!("Custom token must be {}-{} chars", token_min, token_max), env);
            }
            t
        }
        None => crypto::generate_random_string(token_len),
    };

    // Check for duplicate
    if (storage::read_meta(&bucket, &channel_id).await?).is_some() {
        return err_json(409, "channel_id_exists", "This channel ID already exists.", env);
    }

    // Generate passcode + hash
    let passcode = crypto::generate_random_string(pass_len);
    let salt = crypto::generate_salt();
    let passcode_hash = crypto::hash_passcode(&passcode, &salt);

    let now = now_ms();
    let ttl_created = env_var_u64(env, "CHANNEL_TTL_CREATED", 300) * 1000;
    let max_attempts = env_var_usize(env, "MAX_PASSCODE_ATTEMPTS", 5) as u32;

    let meta = ChannelMeta {
        state: storage::STATE_CREATED.to_string(),
        passcode_hash,
        salt_hex: hex::encode(salt),
        created_at: now,
        expires_at: now + ttl_created,
        passcode_attempts: 0,
        max_passcode_attempts: max_attempts,
    };

    storage::write_meta(&bucket, &channel_id, &meta).await?;

    ok_json(json!({
        "channel_id": channel_id,
        "passcode": passcode,
        "created_at": now,
        "expires_at": meta.expires_at
    }), env)
}

/// POST /listen
pub async fn handle_listen(mut req: Request, env: &Env, _ctx: &Context) -> Result<Response> {
    let bucket = storage::get_bucket(env)?;
    let body: ChannelRequest = req.json().await.map_err(|_| worker::Error::RustError("Invalid JSON".into()))?;

    let mut meta = match storage::read_meta(&bucket, &body.channel_id).await? {
        Some(m) => m,
        None => return err_json(404, "channel_not_found", "Channel does not exist.", env),
    };

    // Check expiry
    if now_ms() > meta.expires_at {
        return err_json(410, "channel_expired", "Channel has expired.", env);
    }

    // Verify passcode
    if !verify_passcode(&bucket, &body.channel_id, &mut meta, &body.passcode).await? {
        if meta.passcode_attempts >= meta.max_passcode_attempts {
            return err_json(403, "passcode_locked", "Too many wrong attempts.", env);
        }
        return err_json(401, "invalid_passcode", "Invalid passcode.", env);
    }

    // Check state
    if meta.state != storage::STATE_CREATED {
        return err_json(409, "invalid_state", &format!("Channel is in {} state, expected CREATED.", meta.state), env);
    }

    // Validate + store callee signal
    let signal = match &body.signal {
        Some(s) => s,
        None => return err_json(400, "missing_signal", "Signal data is required.", env),
    };
    let max_signal_bytes = env_var_usize(env, "MAX_SIGNAL_BYTES", 65536);
    if let Some((code, msg)) = validate_signal(signal, max_signal_bytes) {
        let status = if code == "signal_too_large" { 413 } else { 400 };
        return err_json(status, code, msg, env);
    }
    let signal_str = serde_json::to_string(signal).unwrap_or_default();
    storage::write_signal(&bucket, &body.channel_id, "callee", &signal_str).await?;

    // Transition CREATED → WAITING
    let ttl_waiting = env_var_u64(env, "CHANNEL_TTL_WAITING", 300) * 1000;
    meta.state = storage::STATE_WAITING.to_string();
    meta.expires_at = now_ms() + ttl_waiting;
    storage::write_meta(&bucket, &body.channel_id, &meta).await?;

    ok_json(json!({
        "status": "waiting",
        "channel_id": body.channel_id
    }), env)
}

/// POST /join
pub async fn handle_join(mut req: Request, env: &Env, _ctx: &Context) -> Result<Response> {
    let bucket = storage::get_bucket(env)?;
    let body: ChannelRequest = req.json().await.map_err(|_| worker::Error::RustError("Invalid JSON".into()))?;

    let mut meta = match storage::read_meta(&bucket, &body.channel_id).await? {
        Some(m) => m,
        None => return err_json(404, "channel_not_found", "Channel does not exist.", env),
    };

    if now_ms() > meta.expires_at {
        return err_json(410, "channel_expired", "Channel has expired.", env);
    }

    if !verify_passcode(&bucket, &body.channel_id, &mut meta, &body.passcode).await? {
        if meta.passcode_attempts >= meta.max_passcode_attempts {
            return err_json(403, "passcode_locked", "Too many wrong attempts.", env);
        }
        return err_json(401, "invalid_passcode", "Invalid passcode.", env);
    }

    if meta.state == storage::STATE_LOCKED {
        return err_json(403, "channel_locked", "This channel is already in use.", env);
    }
    if meta.state != storage::STATE_WAITING {
        return err_json(409, "invalid_state", &format!("Channel is in {} state, expected WAITING.", meta.state), env);
    }

    // Store caller signal if provided (opaque)
    if let Some(s) = &body.signal {
        let signal_str = serde_json::to_string(s).unwrap_or_default();
        storage::write_signal(&bucket, &body.channel_id, "caller", &signal_str).await?;
    }

    // Read callee signal
    let callee_signal = storage::read_signal(&bucket, &body.channel_id, "callee").await?;

    // Transition WAITING → LOCKED
    let ttl_locked = env_var_u64(env, "CHANNEL_TTL_LOCKED", 3600) * 1000;
    meta.state = storage::STATE_LOCKED.to_string();
    meta.expires_at = now_ms() + ttl_locked;
    storage::write_meta(&bucket, &body.channel_id, &meta).await?;

    let callee_signal_value: serde_json::Value = callee_signal
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null);

    ok_json(json!({
        "status": "locked",
        "callee_signal": callee_signal_value
    }), env)
}

/// POST /poll
pub async fn handle_poll(mut req: Request, env: &Env, ctx: &Context) -> Result<Response> {
    let bucket = storage::get_bucket(env)?;
    let body: ChannelRequest = req.json().await.map_err(|_| worker::Error::RustError("Invalid JSON".into()))?;

    // Opportunistic vacuum: clean up this channel if it has expired (runs after response)
    let bucket_for_vacuum = storage::get_bucket(env)?;
    let vacuum_channel_id = body.channel_id.clone();
    ctx.wait_until(async move {
        vacuum::maybe_vacuum(&bucket_for_vacuum, &vacuum_channel_id).await;
    });

    let mut meta = match storage::read_meta(&bucket, &body.channel_id).await? {
        Some(m) => m,
        None => return err_json(404, "channel_not_found", "Channel does not exist.", env),
    };

    if now_ms() > meta.expires_at {
        return err_json(410, "channel_expired", "Channel has expired.", env);
    }

    if !verify_passcode(&bucket, &body.channel_id, &mut meta, &body.passcode).await? {
        if meta.passcode_attempts >= meta.max_passcode_attempts {
            return err_json(403, "passcode_locked", "Too many wrong attempts.", env);
        }
        return err_json(401, "invalid_passcode", "Invalid passcode.", env);
    }

    if meta.state == storage::STATE_TERMINATED {
        return err_json(410, "channel_terminated", "Channel has been terminated.", env);
    }

    if meta.state == storage::STATE_WAITING {
        return ok_json(json!({ "status": "waiting" }), env);
    }

    if meta.state == storage::STATE_LOCKED {
        // If caller is posting signal, validate and store it
        if body.role == "caller" {
            if let Some(s) = &body.signal {
                let existing = storage::read_signal(&bucket, &body.channel_id, "caller").await?;
                if existing.is_none() {
                    let max_signal_bytes = env_var_usize(env, "MAX_SIGNAL_BYTES", 65536);
                    if let Some((code, msg)) = validate_signal(s, max_signal_bytes) {
                        let status = if code == "signal_too_large" { 413 } else { 400 };
                        return err_json(status, code, msg, env);
                    }
                    let signal_str = serde_json::to_string(s).unwrap_or_default();
                    storage::write_signal(&bucket, &body.channel_id, "caller", &signal_str).await?;
                    return ok_json(json!({ "status": "signal_stored" }), env);
                }
            }
        }

        let caller_signal = storage::read_signal(&bucket, &body.channel_id, "caller").await?;
        let caller_signal_value: serde_json::Value = caller_signal
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(serde_json::Value::Null);
        return ok_json(json!({
            "status": "locked",
            "state": "LOCKED",
            "caller_signal": caller_signal_value
        }), env);
    }

    ok_json(json!({ "status": meta.state }), env)
}

/// POST /hangup
pub async fn handle_hangup(mut req: Request, env: &Env, ctx: &Context) -> Result<Response> {
    let bucket = storage::get_bucket(env)?;
    let body: ChannelRequest = req.json().await.map_err(|_| worker::Error::RustError("Invalid JSON".into()))?;

    let mut meta = match storage::read_meta(&bucket, &body.channel_id).await? {
        Some(m) => m,
        None => return err_json(404, "channel_not_found", "Channel does not exist.", env),
    };

    if !verify_passcode(&bucket, &body.channel_id, &mut meta, &body.passcode).await? {
        if meta.passcode_attempts >= meta.max_passcode_attempts {
            return err_json(403, "passcode_locked", "Too many wrong attempts.", env);
        }
        return err_json(401, "invalid_passcode", "Invalid passcode.", env);
    }

    // Transition → TERMINATED
    meta.state = storage::STATE_TERMINATED.to_string();
    meta.expires_at = now_ms(); // expire immediately
    storage::write_meta(&bucket, &body.channel_id, &meta).await?;

    // Async cleanup
    let channel_id = body.channel_id.clone();
    let bucket_for_cleanup = storage::get_bucket(env)?;
    ctx.wait_until(async move {
        storage::delete_channel(&bucket_for_cleanup, &channel_id).await.ok();
    });

    ok_json(json!({ "status": "terminated" }), env)
}

/// GET /
pub async fn handle_health(env: &Env) -> Result<Response> {
    let body = json!({ "status": "ok", "service": "zrtc" });
    ok_json(body, env)
}

/// OPTIONS *
pub fn handle_options(env: &Env) -> Result<Response> {
    let origin = allowed_origin(env);
    let headers = Headers::new();
    headers.set("Access-Control-Allow-Origin", &origin)?;
    headers.set("Access-Control-Allow-Methods", "GET, POST, OPTIONS")?;
    headers.set("Access-Control-Allow-Headers", "Content-Type")?;
    headers.set("Access-Control-Max-Age", "86400")?;
    Ok(Response::builder()
        .with_status(204)
        .with_headers(headers)
        .body(worker::ResponseBody::Empty)
    )
}

// ── Signal Validation ───────────────────────────────────

/// Validate a signal payload before storing it in R2.
/// Returns Some((error_code, message)) on failure, None on success.
fn validate_signal(signal: &serde_json::Value, max_bytes: usize) -> Option<(&'static str, &'static str)> {
    // Must be a JSON object
    let obj = match signal.as_object() {
        Some(o) => o,
        None => return Some(("invalid_signal", "Signal must be a JSON object.")),
    };

    // Check byte size (re-serialize to get exact byte count)
    let serialized = serde_json::to_string(signal).unwrap_or_default();
    if serialized.len() > max_bytes {
        return Some(("signal_too_large", "Signal payload exceeds maximum allowed size."));
    }

    // Must match SDP descriptor or ICE candidate shape
    let is_sdp = obj.get("type")
        .and_then(|t| t.as_str())
        .map(|t| matches!(t, "offer" | "answer" | "pranswer"))
        .unwrap_or(false)
        && obj.get("sdp")
            .and_then(|s| s.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false);

    let is_ice = obj.get("candidate")
        .and_then(|c| c.as_str())
        .map(|c| !c.is_empty())
        .unwrap_or(false);

    if !is_sdp && !is_ice {
        return Some(("invalid_signal", "Signal must be a WebRTC SDP descriptor or ICE candidate."));
    }

    None
}
