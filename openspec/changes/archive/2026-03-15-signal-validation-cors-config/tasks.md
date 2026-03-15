## 1. wrangler.toml — Add env vars

- [x] 1.1 Add `ALLOWED_ORIGIN = "*"` to `[vars]` in `worker/wrangler.toml`
- [x] 1.2 Add `MAX_SIGNAL_BYTES = "65536"` to `[vars]` in `worker/wrangler.toml`

## 2. handlers.rs — CORS helper

- [x] 2.1 Add `fn allowed_origin(env: &Env) -> String` helper that reads `ALLOWED_ORIGIN` env var and defaults to `"*"`
- [x] 2.2 Update `cors_headers()` to accept `origin: &str` parameter and use it instead of the hardcoded `"*"`
- [x] 2.3 Update `json_response()` to call `allowed_origin(env)` and pass it to the updated `cors_headers()`; update all call-sites (`ok_json`, `err_json`) that need `env` access
- [x] 2.4 Verify all existing route handlers compile after CORS signature changes

## 3. lib.rs — OPTIONS preflight handler

- [x] 3.1 In the main request router in `src/lib.rs`, add a top-level `Method::Options` branch that calls a new `handle_options(env)` handler before any route dispatch
- [x] 3.2 Implement `handle_options(env: &Env)` returning HTTP 204 with headers: `Access-Control-Allow-Origin`, `Access-Control-Allow-Methods: GET, POST, OPTIONS`, `Access-Control-Allow-Headers: Content-Type`, `Access-Control-Max-Age: 86400`

## 4. handlers.rs — Signal validation helper

- [x] 4.1 Implement `fn validate_signal(signal: &serde_json::Value, max_bytes: usize) -> Option<(&'static str, &'static str)>` returning `Some((error_code, message))` on failure or `None` on success
- [x] 4.2 Inside `validate_signal`: check that `signal` is a JSON object
- [x] 4.3 Inside `validate_signal`: re-serialize `signal` to string and check byte length ≤ `max_bytes`; return `("signal_too_large", ...)` if too large (caller maps this to 413)
- [x] 4.4 Inside `validate_signal`: check for SDP shape (`type` ∈ `{"offer","answer","pranswer"}` AND `sdp` is non-empty string) OR ICE candidate shape (`candidate` is non-empty string); return `("invalid_signal", ...)` if neither matches

## 5. handlers.rs — Apply validation in route handlers

- [x] 5.1 In `handle_listen`: read `MAX_SIGNAL_BYTES` from env; call `validate_signal()` on `body.signal` before the R2 write; return 413 for `signal_too_large`, 400 for `invalid_signal`
- [x] 5.2 In `handle_poll` (caller push path, `body.role == "caller"`): read `MAX_SIGNAL_BYTES` from env; call `validate_signal()` before the R2 write; return 413 or 400 accordingly

## 6. Build and verify

- [x] 6.1 Run `cargo check` (or `worker-build`) in `worker/` and confirm zero compilation errors
- [x] 6.2 Manually test `/listen` with an oversized payload returns 413
- [x] 6.3 Manually test `/listen` with non-SDP JSON returns 400
- [x] 6.4 Manually test `OPTIONS /new` returns 204 with correct CORS headers
- [x] 6.5 Verify a valid SDP offer through the full callee→caller flow still succeeds end-to-end
