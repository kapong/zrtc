## 1. Worker: Accept and Store Config at `/new`

- [x] 1.1 Add `require` and `additional` optional fields (`Option<serde_json::Value>`) to `NewChannelRequest` in `handlers.rs`
- [x] 1.2 Add `key_require()` and `key_additional()` R2 key builder functions in `storage.rs`
- [x] 1.3 Add `write_config()` helper in `storage.rs` that writes a `serde_json::Value` blob to an R2 key (skip write if `None`)
- [x] 1.4 Add `read_config()` helper in `storage.rs` that reads an optional JSON blob from an R2 key (returns `None` if key doesn't exist)
- [x] 1.5 In `handle_new()`, after writing meta, write `require` and `additional` blobs to R2 if present

## 2. Worker: Return Config in `/join` and `/poll`

- [x] 2.1 In `handle_join()`, read `require.json` and `additional.json` from R2 and include them in the response alongside `callee_signal`
- [x] 2.2 In `handle_poll()` (callee, LOCKED state), read `require.json` and `additional.json` from R2 and include them in the response alongside `caller_signal`

## 3. Worker: Cleanup Config on Hangup and Vacuum

- [x] 3.1 In `handle_hangup()` / channel deletion, add deletion of `require.json` and `additional.json` R2 keys
- [x] 3.2 In `vacuum.rs` cleanup, add deletion of `require.json` and `additional.json` R2 keys for expired channels

## 4. Client: Pass Config in `create()`

- [x] 4.1 In `signalling.js` `createChannel()`, pass `require` and `additional` from options to the `/new` request body
- [x] 4.2 In `zrtc.js` `create()`, accept `require` and `additional` in options and forward to `createChannel()`

## 5. Client: Expose Config on `join()` and `listen()`

- [x] 5.1 In `zrtc.js`, add `this.require` and `this.additional` instance properties (initialized to `null`)
- [x] 5.2 In `join()`, read `require` and `additional` from the `/join` response and set them on the instance
- [x] 5.3 In `listen()` poll handling, read `require` and `additional` from the LOCKED poll response and set them on the instance

## 6. Verification

- [x] 6.1 Test worker: create channel with config → join → verify config returned in response
- [x] 6.2 Test worker: create channel without config → join → verify `null` config in response
- [x] 6.3 Test worker: hangup → verify config blobs deleted from R2

## 7. Integration Tests (tests/test-worker-api.sh)

- [x] 7.1 Add test: `POST /new` with `require` and `additional` → assert 200, create succeeds
- [x] 7.2 Add test: `POST /listen` + `POST /join` on config channel → assert `require` and `additional` present in `/join` response
- [x] 7.3 Add test: `POST /poll` (callee, LOCKED) on config channel → assert `require` and `additional` present in response
- [x] 7.4 Add test: `POST /new` without config fields → `POST /listen` + `POST /join` → assert `require` and `additional` are `null` in `/join` response
- [x] 7.5 Add test: `POST /hangup` on config channel → `POST /poll` → assert channel gone (404), confirming config blobs cleaned up
- [x] 7.6 Add test: `POST /new` with only `require` (no `additional`) → `/join` → assert `require` present, `additional` is `null`
- [x] 7.7 Run full test suite and verify all existing + new tests pass
