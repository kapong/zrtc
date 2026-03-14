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

fn cors_headers() -> Headers {
    let h = Headers::new();
    h.set("Access-Control-Allow-Origin", "*").ok();
    h.set("Content-Type", "application/json").ok();
    h
}

fn json_response(status: u16, body: serde_json::Value) -> Result<Response> {
    let mut resp = Response::from_json(&body)?;
    let headers = resp.headers_mut();
    headers.set("Access-Control-Allow-Origin", "*")?;
    // worker crate doesn't support setting status directly on from_json,
    // so we recreate with correct status
    Ok(Response::builder()
        .with_status(status)
        .with_headers(cors_headers())
        .body(worker::ResponseBody::Body(serde_json::to_vec(&body).unwrap()))
    )
}

fn ok_json(body: serde_json::Value) -> Result<Response> {
    json_response(200, body)
}

fn err_json(status: u16, error: &str, message: &str) -> Result<Response> {
    json_response(status, json!({ "error": error, "message": message }))
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
        return err_json(400, "invalid_token_length", &format!("token_length must be {}-{}", token_min, token_max));
    }
    if pass_len < pass_min || pass_len > pass_max {
        return err_json(400, "invalid_passcode_length", &format!("passcode_length must be {}-{}", pass_min, pass_max));
    }

    // Generate or validate custom token
    let channel_id = match custom_token {
        Some(t) => {
            if t.len() < token_min || t.len() > token_max {
                return err_json(400, "invalid_token_length", &format!("Custom token must be {}-{} chars", token_min, token_max));
            }
            t
        }
        None => crypto::generate_random_string(token_len),
    };

    // Check for duplicate
    if (storage::read_meta(&bucket, &channel_id).await?).is_some() {
        return err_json(409, "channel_id_exists", "This channel ID already exists.");
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
    }))
}

/// POST /listen
pub async fn handle_listen(mut req: Request, env: &Env, _ctx: &Context) -> Result<Response> {
    let bucket = storage::get_bucket(env)?;
    let body: ChannelRequest = req.json().await.map_err(|_| worker::Error::RustError("Invalid JSON".into()))?;

    let mut meta = match storage::read_meta(&bucket, &body.channel_id).await? {
        Some(m) => m,
        None => return err_json(404, "channel_not_found", "Channel does not exist."),
    };

    // Check expiry
    if now_ms() > meta.expires_at {
        return err_json(410, "channel_expired", "Channel has expired.");
    }

    // Verify passcode
    if !verify_passcode(&bucket, &body.channel_id, &mut meta, &body.passcode).await? {
        if meta.passcode_attempts >= meta.max_passcode_attempts {
            return err_json(403, "passcode_locked", "Too many wrong attempts.");
        }
        return err_json(401, "invalid_passcode", "Invalid passcode.");
    }

    // Check state
    if meta.state != storage::STATE_CREATED {
        return err_json(409, "invalid_state", &format!("Channel is in {} state, expected CREATED.", meta.state));
    }

    // Store callee signal (opaque)
    let signal_str = match &body.signal {
        Some(s) => serde_json::to_string(s).unwrap_or_default(),
        None => return err_json(400, "missing_signal", "Signal data is required."),
    };
    storage::write_signal(&bucket, &body.channel_id, "callee", &signal_str).await?;

    // Transition CREATED → WAITING
    let ttl_waiting = env_var_u64(env, "CHANNEL_TTL_WAITING", 300) * 1000;
    meta.state = storage::STATE_WAITING.to_string();
    meta.expires_at = now_ms() + ttl_waiting;
    storage::write_meta(&bucket, &body.channel_id, &meta).await?;

    ok_json(json!({
        "status": "waiting",
        "channel_id": body.channel_id
    }))
}

/// POST /join
pub async fn handle_join(mut req: Request, env: &Env, _ctx: &Context) -> Result<Response> {
    let bucket = storage::get_bucket(env)?;
    let body: ChannelRequest = req.json().await.map_err(|_| worker::Error::RustError("Invalid JSON".into()))?;

    let mut meta = match storage::read_meta(&bucket, &body.channel_id).await? {
        Some(m) => m,
        None => return err_json(404, "channel_not_found", "Channel does not exist."),
    };

    if now_ms() > meta.expires_at {
        return err_json(410, "channel_expired", "Channel has expired.");
    }

    if !verify_passcode(&bucket, &body.channel_id, &mut meta, &body.passcode).await? {
        if meta.passcode_attempts >= meta.max_passcode_attempts {
            return err_json(403, "passcode_locked", "Too many wrong attempts.");
        }
        return err_json(401, "invalid_passcode", "Invalid passcode.");
    }

    if meta.state == storage::STATE_LOCKED {
        return err_json(403, "channel_locked", "This channel is already in use.");
    }
    if meta.state != storage::STATE_WAITING {
        return err_json(409, "invalid_state", &format!("Channel is in {} state, expected WAITING.", meta.state));
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
    }))
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
        None => return err_json(404, "channel_not_found", "Channel does not exist."),
    };

    if now_ms() > meta.expires_at {
        return err_json(410, "channel_expired", "Channel has expired.");
    }

    if !verify_passcode(&bucket, &body.channel_id, &mut meta, &body.passcode).await? {
        if meta.passcode_attempts >= meta.max_passcode_attempts {
            return err_json(403, "passcode_locked", "Too many wrong attempts.");
        }
        return err_json(401, "invalid_passcode", "Invalid passcode.");
    }

    if meta.state == storage::STATE_TERMINATED {
        return err_json(410, "channel_terminated", "Channel has been terminated.");
    }

    if meta.state == storage::STATE_WAITING {
        return ok_json(json!({ "status": "waiting" }));
    }

    if meta.state == storage::STATE_LOCKED {
        // If caller is posting signal, store it
        if body.role == "caller" {
            if let Some(s) = &body.signal {
                let existing = storage::read_signal(&bucket, &body.channel_id, "caller").await?;
                if existing.is_none() {
                    let signal_str = serde_json::to_string(s).unwrap_or_default();
                    storage::write_signal(&bucket, &body.channel_id, "caller", &signal_str).await?;
                    return ok_json(json!({ "status": "signal_stored" }));
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
        }));
    }

    ok_json(json!({ "status": meta.state }))
}

/// POST /hangup
pub async fn handle_hangup(mut req: Request, env: &Env, ctx: &Context) -> Result<Response> {
    let bucket = storage::get_bucket(env)?;
    let body: ChannelRequest = req.json().await.map_err(|_| worker::Error::RustError("Invalid JSON".into()))?;

    let mut meta = match storage::read_meta(&bucket, &body.channel_id).await? {
        Some(m) => m,
        None => return err_json(404, "channel_not_found", "Channel does not exist."),
    };

    if !verify_passcode(&bucket, &body.channel_id, &mut meta, &body.passcode).await? {
        if meta.passcode_attempts >= meta.max_passcode_attempts {
            return err_json(403, "passcode_locked", "Too many wrong attempts.");
        }
        return err_json(401, "invalid_passcode", "Invalid passcode.");
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

    ok_json(json!({ "status": "terminated" }))
}

/// GET /
pub async fn handle_health(_env: &Env) -> Result<Response> {
    let body = json!({ "status": "ok", "service": "zrtc" });
    ok_json(body)
}

/// OPTIONS *
pub fn handle_options() -> Result<Response> {
    let headers = Headers::new();
    headers.set("Access-Control-Allow-Origin", "*")?;
    headers.set("Access-Control-Allow-Methods", "GET,POST,OPTIONS")?;
    headers.set("Access-Control-Allow-Headers", "Content-Type")?;
    headers.set("Access-Control-Max-Age", "86400")?;
    Ok(Response::empty()?.with_headers(headers))
}
