## ADDED Requirements

### Requirement: Channel creation accepts optional config objects
The `/new` endpoint SHALL accept two optional JSON fields in the request body: `require` (connection requirements) and `additional` (optional settings). Both fields SHALL be arbitrary JSON objects. If not provided, they SHALL default to `null`.

#### Scenario: Create channel with require and additional
- **WHEN** a client sends `POST /new` with body `{ "require": { "max_bitrate": 500000 }, "additional": { "caller_name": "Alice" } }`
- **THEN** the worker SHALL create the channel, store both config objects in R2, and return the standard `{ channel_id, passcode, created_at, expires_at }` response

#### Scenario: Create channel without config fields
- **WHEN** a client sends `POST /new` with body `{}` (no `require` or `additional`)
- **THEN** the worker SHALL create the channel normally and no config blobs SHALL be written to R2

#### Scenario: Create channel with only require
- **WHEN** a client sends `POST /new` with body `{ "require": { "video_output_width": 1280 } }`
- **THEN** the worker SHALL store only the `require` blob; no `additional` blob SHALL be written

### Requirement: Config objects stored as opaque R2 blobs
The worker SHALL store `require` at `channels/{channel_id}/require.json` and `additional` at `channels/{channel_id}/additional.json`. The worker SHALL NOT parse, validate, or inspect the contents of these blobs.

#### Scenario: R2 key layout for config
- **WHEN** a channel is created with both config fields
- **THEN** R2 SHALL contain `channels/{channel_id}/require.json` and `channels/{channel_id}/additional.json` as separate objects alongside `channels/{channel_id}/meta.json`

### Requirement: Join response includes config objects
The `/join` endpoint response SHALL include `require` and `additional` fields containing the stored config objects. If a config object was not provided at creation, the field SHALL be `null` in the response.

#### Scenario: Caller joins channel with config
- **WHEN** a caller sends `POST /join` for a channel created with `{ "require": { "max_bitrate": 500000 }, "additional": { "from_app": "demo" } }`
- **THEN** the response SHALL include `"require": { "max_bitrate": 500000 }` and `"additional": { "from_app": "demo" }` alongside `callee_signal`

#### Scenario: Caller joins channel without config
- **WHEN** a caller sends `POST /join` for a channel created without config fields
- **THEN** the response SHALL include `"require": null` and `"additional": null`

### Requirement: Poll response includes config objects when LOCKED
The `/poll` endpoint (role: callee) response SHALL include `require` and `additional` fields when the channel state is `LOCKED`. For other states, the config fields MAY be omitted.

#### Scenario: Callee polls and gets config at LOCKED
- **WHEN** a callee polls and the channel is in LOCKED state
- **THEN** the response SHALL include `require` and `additional` fields alongside `caller_signal`

### Requirement: Config objects cleaned up with channel
The `require.json` and `additional.json` R2 blobs SHALL be deleted when the channel is deleted (via `/hangup` or vacuum cleanup). The cleanup SHALL follow the same lifecycle as `callee.json` and `caller.json`.

#### Scenario: Hangup deletes config blobs
- **WHEN** a peer calls `/hangup` on a channel that has config objects
- **THEN** the worker SHALL delete `require.json` and `additional.json` alongside `meta.json`, `callee.json`, and `caller.json`

#### Scenario: Vacuum cleans expired channel with config
- **WHEN** the vacuum process finds an expired channel with config blobs
- **THEN** it SHALL delete `require.json` and `additional.json` along with all other channel data

### Requirement: Client create passes config to worker
`ZeroRTC.create()` SHALL accept `require` and `additional` in its options object and pass them to the `/new` API call.

#### Scenario: Client creates channel with requirements
- **WHEN** application code calls `zrtc.create({ require: { max_bitrate: 500000 }, additional: { from_app: "demo" } })`
- **THEN** the client SHALL include both fields in the `POST /new` request body

### Requirement: Client join exposes config objects
`ZeroRTC.join()` SHALL make the `require` and `additional` objects available to application code after joining. The config SHALL be accessible before the WebRTC connection is established.

#### Scenario: Caller reads config after join
- **WHEN** application code calls `zrtc.join(channelId, passcode)` on a channel with config
- **THEN** `zrtc.require` and `zrtc.additional` SHALL be populated with the config objects from the `/join` response

### Requirement: Client listen exposes config objects on LOCKED
When `ZeroRTC.listen()` receives a LOCKED poll response containing config objects, the config SHALL be accessible on the instance.

#### Scenario: Callee reads config when caller joins
- **WHEN** the callee is polling and receives LOCKED state with config
- **THEN** `zrtc.require` and `zrtc.additional` SHALL be populated with the config objects
