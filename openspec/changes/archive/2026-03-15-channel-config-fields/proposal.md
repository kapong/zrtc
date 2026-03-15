## Why

Currently, channel creation (`/new`) only returns `channel_id`, `passcode`, `created_at`, and `expires_at`. There is no way for the channel creator to communicate connection requirements or optional settings to the joining peer before the WebRTC handshake begins. Both peers discover capabilities only after the connection is established, which is too late to enforce constraints like bitrate limits or video resolution, and too late to share helpful context like TURN server configs or caller identity.

## What Changes

- `/new` accepts two new optional open-schema JSON objects: `require` and `additional`
- Worker stores these objects alongside channel metadata in R2 (as separate blobs, kept opaque)
- `/join` and `/poll` (callee role) responses include the stored `require` and `additional` objects so both peers can read them before the WebRTC connection is established
- Client `ZeroRTC.create()` accepts `require` and `additional` options and passes them through
- Client `ZeroRTC.join()` and `ZeroRTC.listen()` expose the received config fields via return values and/or events

## Capabilities

### New Capabilities
- `channel-config`: Storage and retrieval of open-schema `require` and `additional` configuration objects on channels — worker stores them opaquely, client reads and exposes them to application code

### Modified Capabilities
_None — this is additive and does not change existing signal-validation or cors-config behavior._

## Impact

- **Worker API**: `/new` request body gains two optional fields; `/join` and `/poll` responses include them. R2 storage adds up to two new blobs per channel.
- **Client library**: `ZeroRTC.create()` options extended; `join()` and `listen()` return/expose the config objects.
- **Storage**: Two new R2 keys per channel (`channels/{id}/require.json`, `channels/{id}/additional.json`). Opaque blobs, same lifecycle as channel — cleaned up on hangup/vacuum.
- **Breaking changes**: None. Both fields are optional; existing clients and workers continue to work without them.
