use serde::{Deserialize, Serialize};
use worker::*;

/// Channel states as stored in R2.
pub const STATE_CREATED: &str = "CREATED";
pub const STATE_WAITING: &str = "WAITING";
pub const STATE_LOCKED: &str = "LOCKED";
pub const STATE_TERMINATED: &str = "TERMINATED";

/// R2 key builders.
pub fn key_meta(channel_id: &str) -> String {
    format!("channels/{}/meta.json", channel_id)
}

pub fn key_callee(channel_id: &str) -> String {
    format!("channels/{}/callee.json", channel_id)
}

pub fn key_caller(channel_id: &str) -> String {
    format!("channels/{}/caller.json", channel_id)
}

/// Get the R2 bucket from env bindings.
pub fn get_bucket(env: &Env) -> Result<Bucket> {
    env.bucket("BUCKET")
}

/// Channel metadata stored in R2.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChannelMeta {
    pub state: String,
    pub passcode_hash: String,
    pub salt_hex: String,
    pub created_at: u64,
    pub expires_at: u64,
    pub passcode_attempts: u32,
    pub max_passcode_attempts: u32,
}

/// Read channel meta from R2, returns None if not found.
pub async fn read_meta(bucket: &Bucket, channel_id: &str) -> Result<Option<ChannelMeta>> {
    let key = key_meta(channel_id);
    match bucket.get(&key).execute().await? {
        Some(obj) => {
            let text = obj.body().expect("body").text().await?;
            let meta: ChannelMeta =
                serde_json::from_str(&text).map_err(|e| worker::Error::RustError(e.to_string()))?;
            Ok(Some(meta))
        }
        None => Ok(None),
    }
}

/// Write channel meta to R2.
pub async fn write_meta(bucket: &Bucket, channel_id: &str, meta: &ChannelMeta) -> Result<()> {
    let key = key_meta(channel_id);
    let json =
        serde_json::to_string(meta).map_err(|e| worker::Error::RustError(e.to_string()))?;
    bucket.put(&key, json).execute().await?;
    Ok(())
}

/// Read signal blob from R2 (callee or caller).
pub async fn read_signal(bucket: &Bucket, channel_id: &str, role: &str) -> Result<Option<String>> {
    let key = if role == "callee" {
        key_callee(channel_id)
    } else {
        key_caller(channel_id)
    };
    match bucket.get(&key).execute().await? {
        Some(obj) => {
            let text = obj.body().expect("body").text().await?;
            Ok(Some(text))
        }
        None => Ok(None),
    }
}

/// Write signal blob to R2 (opaque, never parsed).
pub async fn write_signal(
    bucket: &Bucket,
    channel_id: &str,
    role: &str,
    blob: &str,
) -> Result<()> {
    let key = if role == "callee" {
        key_callee(channel_id)
    } else {
        key_caller(channel_id)
    };
    bucket.put(&key, blob.to_string()).execute().await?;
    Ok(())
}

/// Delete all channel data from R2.
pub async fn delete_channel(bucket: &Bucket, channel_id: &str) -> Result<()> {
    let keys = vec![
        key_meta(channel_id),
        key_callee(channel_id),
        key_caller(channel_id),
    ];
    for key in keys {
        bucket.delete(&key).await.ok();
    }
    Ok(())
}
