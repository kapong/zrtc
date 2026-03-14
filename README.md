# zrtc — ZeroRTC

Anonymous one-time P2P WebRTC connections via Cloudflare Workers.

No accounts. No IPs logged. No cookies. Signal data is opaque and ephemeral.

---

## How It Works

```
Callee creates channel  →  shares channel ID + passcode (or QR code)
Caller joins channel    →  SDP exchange via worker relay
WebRTC peer connection established  →  relay no longer needed
```

The Cloudflare Worker acts as a **dumb relay** — it stores encrypted SDP blobs in R2 and manages channel state transitions. It never parses signal content. Once the WebRTC connection is established, all data flows directly peer-to-peer.

### Channel State Machine

```
CREATED (5 min) → WAITING (5 min) → LOCKED (1 hr) → TERMINATED
```

---

## Repository Structure

```
worker/     Rust Cloudflare Worker (signalling relay, R2 storage)
client/     JavaScript ES module library (ZeroRTC class)
example/    Vue 3 demo app (Cloudflare Pages)
tests/      API integration tests
```

---

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs) + `wasm32-unknown-unknown` target
- [wrangler](https://developers.cloudflare.com/workers/wrangler/) CLI
- Node.js (LTS)
- A Cloudflare account with an R2 bucket named `zrtc`

### Setup

```bash
# Install all deps and create R2 bucket
make setup

# Copy and fill in your config
cp .env.example .env
```

### Development

```bash
make worker-dev     # Rust worker on http://localhost:8787
make example-dev    # Vue demo on http://localhost:5173
```

### Deploy

```bash
make deploy         # Deploy worker + example in one shot
```

Or step by step:

```bash
make worker-deploy
make example-deploy
```

---

## Client Library

### Install

```bash
npm install zrtc
```

### Usage

```js
import { ZeroRTC, generateQRPayload, parseQRPayload } from 'zrtc'

const WORKER_URL = 'https://your-worker.workers.dev'

// ── Callee side ──────────────────────────────────────────
const callee = new ZeroRTC({ workerUrl: WORKER_URL })

callee.on('connected', () => console.log('Connected!'))
callee.on('data', msg => console.log('Received:', msg))
callee.on('hangup', () => console.log('Call ended'))

const { channelId, passcode } = await callee.create()
await callee.listen()

// Share channelId + passcode (or a QR code) with the caller
const qr = generateQRPayload(WORKER_URL, channelId, passcode)
console.log('Join URL:', qr)

// ── Caller side ──────────────────────────────────────────
const caller = new ZeroRTC({ workerUrl: WORKER_URL })

caller.on('connected', () => console.log('Connected!'))
caller.on('data', msg => console.log('Received:', msg))

await caller.join(channelId, passcode)

// ── Both sides, once connected ───────────────────────────
callee.send('Hello!')
caller.send('Hi back!')

// End the call
await callee.hangup()
```

### Constructor Options

| Option | Default | Description |
|--------|---------|-------------|
| `workerUrl` | required | Base URL of the Cloudflare Worker |
| `stunIceServers` | Google STUN | Custom STUN server list |
| `turnIceServers` | `[]` | TURN servers appended to ICE config |
| `localStream` | `null` | `MediaStream` to send audio/video |
| `fastPollingRateMs` | `500` | Poll interval during connection setup |
| `slowPollingRateMs` | `2000` | Poll interval while waiting for caller |

### Events

| Event | Payload | When |
|-------|---------|------|
| `connected` | — | DataChannel open |
| `joining` | — | Caller just joined (callee side) |
| `data` | message | Incoming DataChannel message |
| `stream` | `MediaStream` | Remote audio/video track received |
| `disconnected` | — | DataChannel or ICE disconnected |
| `hangup` | — | Remote peer hung up |
| `error` | `Error` | Signalling or ICE failure |

### QR Helpers

```js
// Encode a join link into a QR-friendly string
const payload = generateQRPayload(workerUrl, channelId, passcode)
// → "https://your-worker.workers.dev/join#abc123:xyz789"

// Decode it back
const { workerUrl, channelId, passcode } = parseQRPayload(payload)
```

---

## Worker API

All endpoints accept and return JSON. CORS headers are included on every response.

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/` | Health check |
| `POST` | `/new` | Create a channel |
| `POST` | `/new/:token` | Create a channel with a custom token |
| `POST` | `/listen` | Callee posts SDP offer |
| `POST` | `/join` | Caller fetches callee SDP |
| `POST` | `/poll` | Callee polls state; caller pushes SDP answer |
| `POST` | `/hangup` | Terminate channel |

### Create a channel

```
POST /new
{ "token_length": 8, "passcode_length": 6 }

→ { "channel_id": "abc12345", "passcode": "xy9z1q", "created_at": 1700000000000, "expires_at": 1700000300000 }
```

---

## Worker Configuration (`wrangler.toml`)

| Var | Default | Description |
|-----|---------|-------------|
| `TOKEN_LENGTH_DEFAULT` | `8` | Default channel ID length |
| `TOKEN_LENGTH_MIN/MAX` | `6` / `32` | Allowed range |
| `PASSCODE_LENGTH_DEFAULT` | `6` | Default passcode length |
| `PASSCODE_LENGTH_MIN/MAX` | `4` / `8` | Allowed range |
| `CHANNEL_TTL_CREATED` | `300` s | TTL before callee posts signal |
| `CHANNEL_TTL_WAITING` | `300` s | TTL before caller joins |
| `CHANNEL_TTL_LOCKED` | `3600` s | TTL during active call |
| `MAX_PASSCODE_ATTEMPTS` | `5` | Wrong passcode lockout threshold |

---

## Privacy & Security

- No user accounts or identity
- Passcodes hashed with SHA-256 + random salt; verified in constant time
- Signal blobs are opaque to the worker — content is never parsed or logged
- All channel data is deleted from R2 on hangup (or after TTL expiry)
- Opportunistic vacuum cleans up expired channels on each `/poll` request

---

## License

MIT
