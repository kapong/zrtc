## Context

The zrtc worker currently creates channels with only identity/auth metadata (`channel_id`, `passcode`, hashes, TTLs, attempt counters). There is no mechanism for the channel creator to attach configuration that both peers can read before the WebRTC connection is established. Peers must negotiate capabilities out-of-band or after connection, which is too late for constraints like bitrate limits or video resolution.

The worker is a dumb relay — it stores opaque blobs and never interprets their contents. This principle extends to config fields: they are stored and returned as-is.

## Goals / Non-Goals

**Goals:**
- Allow channel creator to attach a `require` object (connection requirements both peers must follow) and an `additional` object (optional/informational settings) at channel creation time
- Store both objects in R2 as opaque blobs alongside existing channel data
- Return both objects to the joining peer (via `/join`) and to the callee (via `/poll` when LOCKED)
- Expose both objects in the client library so application code can read and act on them
- Maintain backward compatibility — both fields are fully optional

**Non-Goals:**
- Worker-side validation or enforcement of config field contents (worker stays dumb)
- Defining a fixed schema for `require` or `additional` — these are open objects defined by the application
- Client-side automatic enforcement of `require` fields (the client exposes them; app code decides what to do)
- Size limits on config fields beyond R2's natural limits (may revisit later with rate limiting)

## Decisions

### 1. Store config as separate R2 blobs (not embedded in meta.json)

**Decision**: Store `require` and `additional` as `channels/{id}/require.json` and `channels/{id}/additional.json`, separate from `meta.json`.

**Rationale**: Keeps `meta.json` focused on channel state machine data (state, hashes, TTLs). Config blobs can be arbitrarily sized and are opaque — embedding them in meta would complicate every read/write of channel metadata.

**Alternative considered**: Embedding in `meta.json` — simpler (fewer R2 ops) but mixes concerns and increases meta read/write payload for every state transition.

### 2. Write config at `/new` time, not at `/listen`

**Decision**: Config fields are provided in the `/new` request body and stored immediately at channel creation.

**Rationale**: The callee creates the channel and knows the requirements upfront. Storing at `/new` ensures config is available before any peer interaction. The callee can read back their own config from `/poll` responses.

**Alternative considered**: Storing at `/listen` time — but `/listen` is for signal posting, and the caller needs config before sending their signal in `/join`.

### 3. Return config in `/join` response and `/poll` (callee, LOCKED state)

**Decision**: `/join` returns `require` and `additional` in its response so the caller gets them immediately. `/poll` (role: callee) returns them when channel transitions to LOCKED so the callee can confirm what was set.

**Rationale**: Both peers need config before establishing WebRTC. The caller gets it at join time; the callee already knows it (they set it) but receiving it back in the LOCKED poll confirms the channel state.

### 4. Open-schema objects with no worker-side validation

**Decision**: `require` and `additional` are `serde_json::Value` (arbitrary JSON). The worker stores and returns them without inspection.

**Rationale**: Follows the "worker is dumb" principle. Different applications will define different config schemas. Validation belongs in client-side application code.

### 5. Client exposes config via return values

**Decision**: `ZeroRTC.create()` passes `require` and `additional` through to the API. `ZeroRTC.join()` returns the config objects. `ZeroRTC.listen()` exposes them when the LOCKED poll returns.

**Rationale**: Minimal API surface. Application code reads the config and decides how to configure the `RTCPeerConnection` (bitrate, resolution, etc.) or display UI (caller name, app info).

## Risks / Trade-offs

- **[Unbounded blob size]** → No size limit on `require`/`additional` could allow abuse. Mitigation: acceptable for now; future rate limiting (Phase 3) will address this. R2 write costs are negligible at small scale.
- **[Two extra R2 reads on join/poll]** → Slight latency increase when fetching config blobs. Mitigation: Config is small JSON; R2 reads within a worker are fast (<5 ms). Only read when config exists.
- **[No enforcement of `require`]** → Peers can ignore requirements. Mitigation: By design — enforcement is application-layer responsibility. The library provides the data; apps decide policy.
- **[Schema evolution]** → Applications may change their config schema over time. Mitigation: Open-schema approach means the worker needs no changes. Client apps handle versioning internally.
