# AGENTS.md — zrtc Project

## Project Overview

Anonymous one-time P2P WebRTC connections via Cloudflare Workers. Two independent codebases:

- **`worker/`** — Rust Cloudflare Worker (dumb relay, R2 storage, zero identity)
- **`client/`** — JavaScript ES module library (handles WebRTC, SDP, polling, chunking)

---

## Architecture Principles

1. **Anonymous by design** — No accounts, no IPs logged, no cookies, signal data opaque, everything ephemeral.
2. **Worker is dumb** — Store blobs, verify hashed passcodes, manage state transitions. Never parse signal content.
3. **Client is smart** — All WebRTC logic, SDP construction, NAT traversal, chunking, reconnect, QR helpers.
4. **HTTP polling only** — No WebSockets, no Durable Objects. Keep cost at Cloudflare free/cheap tier.
5. **Full SDP exchange** — Full offer/answer SDP with ICE candidates embedded by the browser. DataChannel + optional audio/video media tracks.

---

## Worker (`worker/`)

**Language:** Rust (compiled to WASM via `worker-build`)
**Runtime:** Cloudflare Workers
**Storage:** R2 bucket (`zrtc`)

### File Structure
| File | Purpose |
|------|---------|
| `src/lib.rs` | Entry point, request routing |
| `src/handlers.rs` | Route handlers: /new, /listen, /join, /poll, /hangup |
| `src/storage.rs` | R2 read/write/delete helpers, key layout |
| `src/crypto.rs` | Token generation, passcode hashing (SHA-256 + salt), constant-time compare |
| `src/vacuum.rs` | Opportunistic cleanup of expired channels via custom R2 metadata |
| `Cargo.toml` | Rust dependencies |
| `wrangler.toml` | Cloudflare Workers config, R2 binding, env vars |

### API Endpoints
| Method | Path | Action |
|--------|------|--------|
| POST | `/new` | Create channel → returns `{ channel_id, passcode, created_at, expires_at }` |
| POST | `/new/:token` | Create channel with custom token |
| POST | `/listen` | Callee stores signal blob, CREATED→WAITING |
| POST | `/join` | Caller fetches callee signal, WAITING→LOCKED (no caller signal yet) |
| POST | `/poll` | Callee polls for state/caller signal; caller pushes signal (role-based) |
| POST | `/hangup` | Transition→TERMINATED, async delete via `ctx.wait_until()` |
| GET | `/` | Health check → `{ status: "ok", service: "zrtc" }` |

### `/poll` Dual Role
- `role: "callee"` — Poll for channel state. When LOCKED, returns `{ state: "LOCKED", caller_signal }`.
- `role: "caller"` + `signal` body — Push caller SDP signal (stored once, idempotent).

### Channel State Machine
```
CREATED (5 min TTL) → WAITING (5 min TTL) → LOCKED (1 h TTL) → TERMINATED
```

### R2 Key Layout
```
channels/{channel_id}/meta.json      ← ChannelMeta (state, hash, TTLs, attempts)
channels/{channel_id}/callee.json    ← callee SDP signal blob (opaque)
channels/{channel_id}/caller.json    ← caller SDP signal blob (opaque)
```

### Config (wrangler.toml `[vars]`)
| Var | Default | Purpose |
|-----|---------|---------|
| `TOKEN_LENGTH_DEFAULT` | 8 | Default channel ID length |
| `TOKEN_LENGTH_MIN/MAX` | 6/32 | Allowed range |
| `PASSCODE_LENGTH_DEFAULT` | 6 | Default passcode length |
| `PASSCODE_LENGTH_MIN/MAX` | 4/8 | Allowed range |
| `CHANNEL_TTL_CREATED` | 300 s | TTL before callee posts signal |
| `CHANNEL_TTL_WAITING` | 300 s | TTL before caller joins |
| `CHANNEL_TTL_LOCKED` | 3600 s | TTL during active call |
| `MAX_PASSCODE_ATTEMPTS` | 5 | Lockout threshold |

### Worker Rules
- Never log IPs or request metadata beyond CORS headers
- Never parse or inspect signal blob contents
- Always hash passcodes with SHA-256 + random salt before storing; constant-time compare
- Enforce max passcode attempts (lockout → 403)
- Enforce token/passcode length limits from wrangler.toml env vars
- Use `ctx.wait_until()` for async channel deletion in `/hangup`
- Return appropriate CORS headers (`Access-Control-Allow-Origin: *`) on all responses
- Single R2 bucket stores everything under `channels/{channel_id}/`

---

## Client (`client/`)

**Language:** JavaScript (ES module)
**Build:** esbuild bundle → `dist/zrtc.js`
**Package name:** `@kapong/zrtc` (local dev); intended as `zrtc` on npm
**Export:** `ZeroRTC` class + QR helpers

### File Structure
| File | Purpose |
|------|---------|
| `src/index.js` | Public exports: `ZeroRTC`, `generateQRPayload`, `parseQRPayload` |
| `src/zrtc.js` | `ZeroRTC` class — lifecycle, events, polling loop, hangup marker |
| `src/signalling.js` | HTTP fetch to worker API: `createChannel`, `listen`, `join`, `pushSignal`, `poll`, `hangup` |
| `src/webrtc.js` | Native `RTCPeerConnection` — full SDP offer/answer, ICE gathering, DataChannel + media |
| `src/chunking.js` | DataChannel message chunking (>16 KB) using P2PCF-compatible binary protocol |
| `src/qr.js` | `generateQRPayload` / `parseQRPayload` — encode/decode `workerUrl/join#channelId:passcode` |
| `package.json` | NPM config, esbuild scripts |

### Signal Flow
1. **Callee** calls `create()` → gets `{ channelId, passcode }`
2. **Callee** calls `listen()` → creates `RTCPeerConnection`, generates full SDP offer (with ICE candidates embedded), posts to `/listen`
3. **Callee** polls `/poll` (role: callee) waiting for `state: "LOCKED"`
4. **Caller** calls `join(channelId, passcode)` → posts to `/join`, receives callee SDP
5. **Caller** creates answer SDP, posts to `/poll` (role: caller) via `pushSignal()`
6. **Callee** poll returns `caller_signal`, finalizes connection → DataChannel opens → `'connected'` emitted

### `ZeroRTC` Constructor Options
| Option | Default | Purpose |
|--------|---------|---------|
| `workerUrl` | required | Base URL of the Cloudflare Worker |
| `stunIceServers` | Google STUN | Custom STUN server list |
| `turnIceServers` | none | Optional TURN servers appended to ICE config |
| `localStream` | null | MediaStream to send to peer |
| `fastPollingRateMs` | 500 | Poll interval during connection setup |
| `slowPollingRateMs` | 2000 | Poll interval while waiting for caller |

### Events
| Event | Payload | When |
|-------|---------|------|
| `'connected'` | — | DataChannel opens |
| `'joining'` | — | Poll returns LOCKED (caller just joined) |
| `'data'` | message | Incoming DataChannel message (chunked reassembled) |
| `'stream'` | MediaStream | Remote audio/video track received |
| `'disconnected'` | — | DataChannel or ICE disconnected |
| `'hangup'` | — | Remote peer sent hangup marker or called `hangup()` |
| `'error'` | Error | Signalling or ICE failure |

### Client Rules
- Custom browser-compatible `EventEmitter` (no Node.js `events` polyfill)
- QR rendering/scanning is UI layer responsibility — library only encodes/decodes the payload string
- Adaptive polling: `fastPollingRateMs` (500 ms) during connection setup, `slowPollingRateMs` (2000 ms) while waiting
- Hangup sends `[0xFF, 0x00]` 2-byte marker over DataChannel, then closes PC and calls `/hangup`
- Caller pushes signal via `/poll` (role: caller) — `/join` only retrieves callee signal
- DataChannel binary type must be `'arraybuffer'`

---

## Implementation Status

### Phase 1: Core (MVP) ✅
- Worker: `/new`, `/listen`, `/join`, `/poll`, `/hangup` with R2 storage
- Worker: Passcode hashing + constant-time verification
- Client: `ZeroRTC.create()`, `.listen()`, `.join()`, `.send()`, `.hangup()`
- Client: Full SDP offer/answer + ICE gathering via native `RTCPeerConnection`

### Phase 2: Robustness ✅
- Worker: `/hangup`, vacuum cleanup
- Client: Chunking (P2PCF-compatible binary protocol), adaptive polling
- Client: QR payload helpers (`generateQRPayload` / `parseQRPayload`)
- Client: Hangup marker, media track support (`localStream`, `'stream'` event)

### Phase 3: Polish (in progress)
- Client: TURN server support (via constructor option + example UI override)
- Worker: Rate limiting — not yet implemented
- End-to-end automated testing — not yet implemented

---

## Example App (`example/`)

**Stack:** Vue 3 + Vite (static SPA)
**Deploy target:** Cloudflare Pages

A minimal P2P calling demo that imports the client library directly from `../../client/src/index.js`. Shows callee creating a channel + displaying a QR code, caller joining by scanning or entering channel_id + passcode. Includes video/audio stream exchange and TURN server override.

### File Structure
| File | Purpose |
|------|---------|
| `package.json` | Vue 3 + Vite deps, `qrcode` for QR rendering, dev/build/deploy scripts |
| `index.html` | Vite entry HTML |
| `vite.config.js` | Vite config with Vue plugin |
| `src/main.js` | Vue app mount |
| `src/App.vue` | Main UI: create/join call, QR code, video display, hangup, TURN override |
| `src/style.css` | Minimal styling |

### Environment Variables
| Var | Default | Purpose |
|-----|---------|---------|
| `VITE_WORKER_URL` | (see `.DEV_INFO.md`) | Worker base URL |

### Running Locally
```bash
cd example
npm install
npm run dev          # Vite dev server on http://localhost:5173
```

### Deploy to Cloudflare Pages
```bash
cd example
npm run build        # outputs to dist/
npx wrangler pages deploy dist/ --project-name=zrtc-demo
```
Or connect the repo to Cloudflare Pages dashboard with:
- Build command: `cd example && npm install && npm run build`
- Build output directory: `example/dist`

---

## Publishing Client to npm (From Zero)

Step-by-step guide to publish the `zrtc` client library as an npm package.

### 1. Prerequisites
```bash
# Install Node.js (LTS) if not already installed
# https://nodejs.org or via nvm:
nvm install --lts

# Verify
node -v && npm -v
```

### 2. Create npm Account
```bash
# Sign up at https://www.npmjs.com/signup (one-time)
# Then login from terminal:
npm login
# Enter username, password, email, and OTP if 2FA enabled
# Verify:
npm whoami
```

### 3. Package.json Setup
The `client/package.json` must have these fields for npm publishing:
```json
{
  "name": "zrtc",
  "version": "0.1.0",
  "description": "Anonymous one-time P2P WebRTC connections via Cloudflare Workers",
  "type": "module",
  "main": "dist/zrtc.js",
  "module": "dist/zrtc.js",
  "exports": {
    ".": {
      "import": "./dist/zrtc.js",
      "default": "./dist/zrtc.js"
    }
  },
  "files": ["dist/", "src/", "README.md", "LICENSE"],
  "repository": { "type": "git", "url": "https://github.com/YOUR_USER/zrtc" },
  "keywords": ["p2p", "webrtc", "anonymous", "cloudflare-workers", "signalling"],
  "license": "MIT"
}
```
Key fields:
- **`name`** — Must be unique on npm. Check with `npm search zrtc`
- **`version`** — Semver. Bump before each publish
- **`files`** — What gets included in the npm tarball (keeps it small)
- **`exports`** — Modern Node.js module resolution
- **`main`** / **`module`** — Entry points for bundlers and Node

### 4. Build Before Publishing
```bash
cd client
npm install
npm run build        # esbuild → dist/zrtc.js
```

### 5. Dry Run (Check What Gets Published)
```bash
npm pack --dry-run   # Shows files that would be included
# Review the list — should only be dist/, src/, README.md, LICENSE
```

### 6. Publish
```bash
# First publish:
npm publish --access public
# (--access public is needed for scoped packages like @yourname/zrtc)

# Subsequent publishes:
# 1. Bump version:
npm version patch    # 0.1.0 → 0.1.1 (bug fix)
npm version minor    # 0.1.0 → 0.2.0 (new feature)
npm version major    # 0.1.0 → 1.0.0 (breaking change)
# 2. Publish:
npm publish
```

### 7. Verify
```bash
# Check it's live:
npm info zrtc

# Test install in another project:
mkdir /tmp/test-zrtc && cd /tmp/test-zrtc
npm init -y
npm install zrtc
node -e "import('zrtc').then(m => console.log(Object.keys(m)))"
```

### 8. Unpublish (Emergency Only)
```bash
# Within 72 hours of publish:
npm unpublish zrtc@0.1.0
# After 72 hours: use npm deprecate instead
npm deprecate zrtc@0.1.0 "Critical bug, use 0.1.1"
```

### npm Publishing Checklist
- [ ] `npm login` done
- [ ] `name` is unique on npm
- [ ] `version` bumped
- [ ] `files` field set (no junk in tarball)
- [ ] `npm run build` succeeds
- [ ] `npm pack --dry-run` looks clean
- [ ] README.md exists in client/
- [ ] LICENSE exists in client/
- [ ] `npm publish --access public`

---

## Deploying Worker to Cloudflare

```bash
cd worker
# First time: install wrangler globally
npm install -g wrangler

# Login to Cloudflare
wrangler login

# Create R2 bucket (one-time)
wrangler r2 bucket create zrtc

# Deploy
wrangler deploy
```

---

## Coding Guidelines

- Keep worker code minimal — it's a dumb relay
- No unnecessary abstractions or over-engineering
- Test each endpoint independently before wiring up client
- Build and `npm pack --dry-run` before publishing client to npm
- Deploy example to Cloudflare Pages after verifying locally
