## ADDED Requirements

### Requirement: Signal payload size is capped before storage
The worker SHALL reject any signal payload whose serialized byte size exceeds the value of the `MAX_SIGNAL_BYTES` environment variable (default 65 536). Rejection SHALL occur before any R2 write.

#### Scenario: Payload within size limit is accepted
- **WHEN** a `/listen` or `/poll` (caller push) request contains a `signal` field whose serialized size is ≤ `MAX_SIGNAL_BYTES`
- **THEN** the worker SHALL proceed with normal processing and return 2xx

#### Scenario: Payload exceeding size limit is rejected
- **WHEN** a `/listen` or `/poll` (caller push) request contains a `signal` field whose serialized size exceeds `MAX_SIGNAL_BYTES`
- **THEN** the worker SHALL return HTTP 413 with JSON body `{ "error": "signal_too_large", "message": "..." }` and SHALL NOT write anything to R2

### Requirement: Signal payload must be a valid WebRTC descriptor
The worker SHALL reject any `signal` value whose top-level JSON structure does not match a recognised WebRTC signal shape. Accepted shapes are:
- **SDP descriptor**: object with `type` ∈ `{"offer", "answer", "pranswer"}` AND `sdp` is a non-empty string.
- **ICE candidate**: object with `candidate` that is a non-empty string.

#### Scenario: SDP offer signal is accepted
- **WHEN** the `signal` field is `{ "type": "offer", "sdp": "<valid sdp string>" }`
- **THEN** the worker SHALL accept the payload and continue with normal processing

#### Scenario: SDP answer signal is accepted
- **WHEN** the `signal` field is `{ "type": "answer", "sdp": "<valid sdp string>" }`
- **THEN** the worker SHALL accept the payload and continue with normal processing

#### Scenario: ICE candidate signal is accepted
- **WHEN** the `signal` field is `{ "candidate": "<ice string>", "sdpMid": "0" }`
- **THEN** the worker SHALL accept the payload and continue with normal processing

#### Scenario: Arbitrary non-SDP JSON is rejected
- **WHEN** the `signal` field is a JSON object that does not contain a recognised WebRTC shape (e.g., `{ "foo": "bar" }`)
- **THEN** the worker SHALL return HTTP 400 with JSON body `{ "error": "invalid_signal", "message": "..." }` and SHALL NOT write anything to R2

#### Scenario: Signal field is not a JSON object
- **WHEN** the `signal` field is a string, number, or array rather than an object
- **THEN** the worker SHALL return HTTP 400 with `{ "error": "invalid_signal", "message": "..." }` and SHALL NOT write anything to R2

### Requirement: `MAX_SIGNAL_BYTES` is configurable at deploy time
The worker SHALL read the maximum signal size from the `MAX_SIGNAL_BYTES` `[vars]` entry in `wrangler.toml`. When the variable is absent the worker SHALL default to 65 536 bytes.

#### Scenario: Custom byte limit is respected
- **WHEN** `MAX_SIGNAL_BYTES` is set to `"8192"` in wrangler.toml and a signal of 9 000 bytes is submitted
- **THEN** the worker SHALL return HTTP 413 and reject the payload
