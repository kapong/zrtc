## Why

The worker currently accepts any arbitrary blob as a signal payload and stores it in R2 without validation — this opens the bucket to abuse (storing large files, non-SDP content). CORS is also hardcoded to `*`, preventing operators from restricting origin access at deploy time.

## What Changes

- Add signal payload validation on `/listen` and `/poll` (caller push): reject payloads that exceed a configurable byte limit and that do not match expected SDP/ICE structure.
- Add a configurable `ALLOWED_ORIGIN` env var in `wrangler.toml`; use it for the `Access-Control-Allow-Origin` response header instead of the current hardcoded `*`. Default remains `*` for open deployments.
- Add a configurable `MAX_SIGNAL_BYTES` env var (default `65536` — 64 KB) to cap signal size.

## Capabilities

### New Capabilities

- `signal-validation`: Validate incoming signal payloads on `/listen` and `/poll` (caller push) — enforce max byte size and require the body to be valid JSON containing an SDP `type`/`sdp` field (or `candidate` field for trickle ICE). Reject with `400` on failure.
- `cors-config`: Replace hardcoded `Access-Control-Allow-Origin: *` with a value driven by the `ALLOWED_ORIGIN` wrangler env var. Include preflight (`OPTIONS`) handling that also respects this setting.

### Modified Capabilities

## Impact

- `worker/src/handlers.rs` — validation logic added to `/listen` and `/poll` handlers; CORS header construction updated across all routes.
- `worker/wrangler.toml` — two new `[vars]`: `MAX_SIGNAL_BYTES`, `ALLOWED_ORIGIN`.
- No client-side changes required; rejection surfaces as existing `error` event via non-2xx HTTP responses.
- No breaking changes to callers sending valid WebRTC SDP signals.
