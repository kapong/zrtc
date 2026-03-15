## Context

The worker currently stores any signal payload verbatim in R2 with no size cap or structural check, creating an abuse vector where any client can write arbitrary large or irrelevant data to the shared bucket. CORS is hardcoded to `Access-Control-Allow-Origin: *` in two places (`cors_headers()` and `json_response()`), with no preflight (`OPTIONS`) handling, making it impossible to restrict origins per deployment.

## Goals / Non-Goals

**Goals:**
- Reject signal payloads above a configurable byte limit (`MAX_SIGNAL_BYTES`) with HTTP 413.
- Reject signal payloads whose top-level JSON structure is not a valid WebRTC SDP descriptor (`{type, sdp}`) or ICE candidate (`{candidate, sdpMid}`) with HTTP 400.
- Make `Access-Control-Allow-Origin` configurable via `ALLOWED_ORIGIN` env var (default `*`).
- Handle `OPTIONS` preflight correctly using the same `ALLOWED_ORIGIN` value.

**Non-Goals:**
- Deep SDP syntax validation (parsing `sdp` string content) — worker stays a dumb relay.
- Per-route or per-channel CORS policies.
- Client-side changes — existing clients remain compatible.

## Decisions

### D1: Validate signal size by raw byte count of the re-serialized signal field

**Options considered:**
- A. Check `Content-Length` header of the full request body.
- B. Read the raw request bytes and cap the entire body.
- C. After JSON parsing, re-serialize the `signal` value to a string and check its byte length.

**Chosen: C.** The `signal` field is nested inside a JSON body that also contains `channel_id` and `passcode`. Capping the entire request body would conflate unrelated fields. Re-serializing the already-parsed `serde_json::Value` to a string gives an accurate byte footprint of just the signal payload and requires no extra I/O. In practice `serde_json::to_string` on a parsed value is cheap and deterministic.

Handlers affected: `/listen` (callee signal) and `/poll` with `role: "caller"` (caller signal).

### D2: Validate signal structure by checking top-level JSON keys

Require the `signal` JSON object to contain exactly one of:
- **SDP descriptor**: `type` ∈ `{"offer","answer","pranswer"}` AND `sdp` is a non-empty string.
- **ICE candidate** (trickle ICE, future compat): `candidate` is a non-empty string.

This is the minimum check to confirm the payload is WebRTC-related. We intentionally do **not** parse the `sdp` string itself — that would violate the "worker is dumb" principle and is not needed for abuse prevention.

Return HTTP 400 `invalid_signal` if neither shape matches.

### D3: Introduce a shared `validate_signal()` helper

Both `/listen` and `/poll` (caller push path) perform the same two checks. Extract to a free function `validate_signal(signal: &serde_json::Value, max_bytes: usize) -> Option<&'static str>` returning an error message string or `None` on success. This keeps the logic in one place and avoids duplication.

### D4: Centralise CORS origin via a helper `allowed_origin(env)` + handle OPTIONS globally

Replace the two hardcoded `"*"` strings with a helper that reads `ALLOWED_ORIGIN` from env (defaulting to `"*"`). Update `cors_headers()` to accept the origin string. Handle `OPTIONS` in `lib.rs` router before dispatching to route handlers — respond with 204 + full CORS preflight headers.

`ALLOWED_ORIGIN` is a **deploy-time** env var in `[vars]` in `wrangler.toml`; operators override it per deployment. No runtime mutation needed.

## Risks / Trade-offs

- **Re-serialization byte count ≠ original bytes** — `serde_json::to_string` may produce slightly different whitespace than the client sent (no indentation). For the purpose of an abuse cap this is acceptable; the difference is negligible vs a meaningful `MAX_SIGNAL_BYTES` value.
- **Strict SDP-type check may reject future WebRTC signal shapes** — mitigated by keeping the check broad (ICE candidate allowed as alternative) and making `MAX_SIGNAL_BYTES` the primary abuse gate.
- **Hardcoded `ALLOWED_ORIGIN = "*"` default** — preserves backward compatibility for existing open deployments; operators who want restriction must explicitly set the var.

## Migration Plan

1. Add `ALLOWED_ORIGIN = "*"` and `MAX_SIGNAL_BYTES = "65536"` to `[vars]` in `wrangler.toml`.
2. Implement `validate_signal()` helper in `handlers.rs`.
3. Update `cors_headers()` to accept `allowed_origin: &str`; update all call-sites to pass the env-derived value.
4. Add `OPTIONS` preflight handler in `lib.rs` router.
5. Apply `validate_signal()` in `/listen` and `/poll` (caller push) handlers.
6. `wrangler deploy` — no R2 schema changes, no state migration needed.

**Rollback:** revert `handlers.rs` and `wrangler.toml` and redeploy. No persistent state is affected.

## Open Questions

- Should `MAX_SIGNAL_BYTES` apply to the **callee** signal only, or also to the caller signal on `/poll`? → Apply to both; symmetric abuse prevention.
- What is a reasonable default for `MAX_SIGNAL_BYTES`? → 65 536 bytes (64 KB). A typical full-SDP offer with all ICE candidates is well under 8 KB; 64 KB leaves headroom for future extensions while blocking bulk uploads.
