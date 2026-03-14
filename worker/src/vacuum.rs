use worker::*;

/// Opportunistic vacuum: delete expired channel data.
/// Called via ctx.wait_until() so it doesn't block the response.
pub async fn maybe_vacuum(bucket: &Bucket, channel_id: &str) {
    // List all objects under channels/{channel_id}/
    let prefix = format!("channels/{}/", channel_id);
    if let Ok(list) = bucket.list().prefix(&prefix).execute().await {
        let now_ms = js_sys::Date::now() as u64;
        for obj in list.objects() {
            // Check custom metadata for expireAt
            if let Ok(meta) = obj.custom_metadata() {
                if let Some(expire_str) = meta.get("expireAt") {
                    if let Ok(expire_at) = expire_str.parse::<u64>() {
                        if expire_at < now_ms {
                            bucket.delete(&obj.key()).await.ok();
                        }
                    }
                }
            }
        }
    }
}
