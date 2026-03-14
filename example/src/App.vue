<script setup>
import { ref, onMounted, onUnmounted } from 'vue'
import { ZeroRTC, generateQRPayload, parseQRPayload } from '../../client/src/index.js'
import QRCode from 'qrcode'

// Media state
const localStream = ref(null)
const mediaReady = ref(false)
const mediaError = ref('')
const localVideo = ref(null)

const WORKER_URL = import.meta.env.VITE_WORKER_URL || 'https://p2p.ldns.me'

// App state
const mode = ref(null)           // 'callee' | 'caller'
const status = ref('idle')       // idle | creating | waiting | joining | connected | error
const statusText = ref('')
const errorText = ref('')

// Callee state
const channelId = ref('')
const passcode = ref('')

// Caller state
const joinChannelId = ref('')
const joinPasscode = ref('')

// QR state
const qrDataUrl = ref('')
const joinUrl = ref('')

// TURN override
const showTurnOverride = ref(false)
const turnUrl = ref('')
const turnUsername = ref('')
const turnCredential = ref('')

// Connection state
const messages = ref([])
const msgInput = ref('')
const remoteStream = ref(null)
const remoteVideo = ref(null)
const connectedLocalVideo = ref(null)
let call = null

// Connection progress
const peerJoining = ref(false)
const callEnded = ref(false)

// Encode TURN config to URL-safe base64
function encodeTurn () {
  if (!turnUrl.value) return ''
  const obj = { u: turnUrl.value }
  if (turnUsername.value) obj.n = turnUsername.value
  if (turnCredential.value) obj.c = turnCredential.value
  return btoa(JSON.stringify(obj)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
}

// Decode TURN config from URL-safe base64
function decodeTurn (s) {
  if (!s) return null
  try {
    const b64 = s.replace(/-/g, '+').replace(/_/g, '/')
    const obj = JSON.parse(atob(b64))
    if (!obj.u) return null
    const server = { urls: obj.u }
    if (obj.n) server.username = obj.n
    if (obj.c) server.credential = obj.c
    return [server]
  } catch { return null }
}

// Parsed TURN from URL (for caller)
let urlTurnServers = null

// localStorage helpers for TURN
function saveTurnToStorage () {
  const obj = {}
  if (turnUrl.value) obj.u = turnUrl.value
  if (turnUsername.value) obj.n = turnUsername.value
  if (turnCredential.value) obj.c = turnCredential.value
  if (obj.u) {
    localStorage.setItem('zrtc_turn', JSON.stringify(obj))
  } else {
    localStorage.removeItem('zrtc_turn')
  }
}

function loadTurnFromStorage () {
  try {
    const raw = localStorage.getItem('zrtc_turn')
    if (!raw) return
    const obj = JSON.parse(raw)
    if (obj.u) turnUrl.value = obj.u
    if (obj.n) turnUsername.value = obj.n
    if (obj.c) turnCredential.value = obj.c
  } catch {}
}

async function requestMedia () {
  try {
    mediaError.value = ''
    const stream = await navigator.mediaDevices.getUserMedia({ video: true, audio: true })
    localStream.value = stream
    mediaReady.value = true
    // Attach to video element after next tick
    setTimeout(() => {
      if (localVideo.value) {
        localVideo.value.srcObject = stream
      }
    }, 50)
  } catch (err) {
    mediaError.value = err.name === 'NotAllowedError'
      ? 'Camera/microphone permission denied. Please allow access and try again.'
      : `Media error: ${err.message}`
    mediaReady.value = false
  }
}

function stopMedia () {
  if (localStream.value) {
    localStream.value.getTracks().forEach(t => t.stop())
    localStream.value = null
  }
  mediaReady.value = false
}

function selectMode (m) {
  mode.value = m
  reset()
}

function reset () {
  status.value = 'idle'
  statusText.value = ''
  errorText.value = ''
  channelId.value = ''
  passcode.value = ''
  joinChannelId.value = ''
  joinPasscode.value = ''
  messages.value = []
  msgInput.value = ''
  mediaError.value = ''
  remoteStream.value = null
  peerJoining.value = false
  callEnded.value = false
  qrDataUrl.value = ''
  joinUrl.value = ''
  if (call) {
    call.hangup()
    call = null
  }
  stopMedia()
}

function wireEvents () {
  call.on('connected', () => {
    status.value = 'connected'
    statusText.value = 'Connected!'
    peerJoining.value = false
    // Re-attach local stream to the connected view's video element
    setTimeout(() => {
      if (connectedLocalVideo.value && localStream.value) {
        connectedLocalVideo.value.srcObject = localStream.value
      }
      if (remoteVideo.value && remoteStream.value) {
        remoteVideo.value.srcObject = remoteStream.value
      }
    }, 50)
  })
  call.on('stream', (stream) => {
    console.log('[App] Remote stream received, tracks:', stream.getTracks().length)
    remoteStream.value = stream
    setTimeout(() => {
      if (remoteVideo.value) {
        remoteVideo.value.srcObject = stream
      }
    }, 50)
  })
  call.on('data', (data) => {
    const text = typeof data === 'string' ? data : new TextDecoder().decode(data)
    messages.value.push({ text, self: false })
  })
  call.on('disconnected', () => {
    statusText.value = 'Disconnected — connection lost'
  })
  call.on('hangup', () => {
    call = null
    stopMedia()
    peerJoining.value = false
    callEnded.value = true
    status.value = 'ended'
    statusText.value = ''
  })
  call.on('joining', () => {
    peerJoining.value = true
    statusText.value = ''
  })
  call.on('error', (err) => {
    errorText.value = err.message || String(err)
  })
}

// ── Callee flow ──
async function createCall () {
  try {
    status.value = 'creating'
    statusText.value = 'Creating channel...'
    errorText.value = ''

    call = new ZeroRTC({
      workerUrl: WORKER_URL,
      localStream: localStream.value || undefined,
      turnIceServers: turnUrl.value ? [{
        urls: turnUrl.value,
        username: turnUsername.value || undefined,
        credential: turnCredential.value || undefined
      }] : undefined
    })
    saveTurnToStorage()
    console.log('[App] createCall turnIceServers:', JSON.stringify(call.turnIceServers))
    wireEvents()

    const creds = await call.create()
    channelId.value = creds.channelId
    passcode.value = creds.passcode

    status.value = 'waiting'
    statusText.value = 'Scan the QR code or share the link below'

    // Generate join URL and QR code (include TURN if set)
    const turnPart = encodeTurn()
    const hash = turnPart
      ? `${creds.channelId}/${creds.passcode}/${turnPart}`
      : `${creds.channelId}/${creds.passcode}`
    const url = `${window.location.origin}${window.location.pathname}#${hash}`
    joinUrl.value = url
    qrDataUrl.value = await QRCode.toDataURL(url, { width: 256, margin: 2, color: { dark: '#000', light: '#fff' } })

    // listen() will resolve when caller joins and WebRTC connects
    await call.listen()
  } catch (err) {
    errorText.value = err.message || String(err)
    status.value = 'error'
  }
}

// ── Caller flow ──
async function joinCall () {
  if (!joinChannelId.value || !joinPasscode.value) return

  try {
    status.value = 'joining'
    statusText.value = 'Joining channel...'
    errorText.value = ''

    call = new ZeroRTC({
      workerUrl: WORKER_URL,
      localStream: localStream.value || undefined,
      turnIceServers: urlTurnServers || (turnUrl.value ? [{
        urls: turnUrl.value,
        username: turnUsername.value || undefined,
        credential: turnCredential.value || undefined
      }] : undefined)
    })
    console.log('[App] joinCall urlTurnServers:', JSON.stringify(urlTurnServers))
    console.log('[App] joinCall turnIceServers:', JSON.stringify(call.turnIceServers))
    wireEvents()

    await call.join(joinChannelId.value, joinPasscode.value)
  } catch (err) {
    errorText.value = err.message || String(err)
    status.value = 'error'
  }
}

function sendMessage () {
  if (!msgInput.value.trim() || !call) return
  const text = msgInput.value.trim()
  call.send(text)
  messages.value.push({ text, self: true })
  msgInput.value = ''
}

function hangup () {
  if (call) {
    call.hangup()
    call = null
  }
  stopMedia()
  peerJoining.value = false
  callEnded.value = true
  status.value = 'ended'
  statusText.value = ''
}

function startNew () {
  callEnded.value = false
  peerJoining.value = false
  mode.value = null
  status.value = 'idle'
  statusText.value = ''
  errorText.value = ''
  channelId.value = ''
  passcode.value = ''
  joinChannelId.value = ''
  joinPasscode.value = ''
  messages.value = []
  remoteStream.value = null
  qrDataUrl.value = ''
  joinUrl.value = ''
}

function closeTab () {
  window.close()
}

function copyLink () {
  if (joinUrl.value) {
    navigator.clipboard.writeText(joinUrl.value)
    statusText.value = 'Link copied!'
    setTimeout(() => {
      if (status.value === 'waiting') statusText.value = 'Scan the QR code or share the link below'
    }, 1500)
  }
}

// Auto-fill from URL hash (e.g. #channelId/passcode or #channelId/passcode/turnBase64)
onMounted(() => {
  // Load saved TURN settings from localStorage
  loadTurnFromStorage()

  // Check ?t= query param for TURN override (applies to any mode)
  const params = new URLSearchParams(window.location.search)
  const tParam = params.get('t')
  if (tParam) {
    const decoded = decodeTurn(tParam)
    if (decoded) {
      urlTurnServers = decoded
      // Also fill in the TURN fields for visibility
      const b64 = tParam.replace(/-/g, '+').replace(/_/g, '/')
      try {
        const obj = JSON.parse(atob(b64))
        if (obj.u) turnUrl.value = obj.u
        if (obj.n) turnUsername.value = obj.n
        if (obj.c) turnCredential.value = obj.c
        saveTurnToStorage()
      } catch {}
      console.log('[App] Decoded TURN from ?t= param:', JSON.stringify(urlTurnServers))
    }
    // Strip ?t= from URL
    params.delete('t')
    const qs = params.toString()
    const clean = window.location.pathname + (qs ? '?' + qs : '') + window.location.hash
    history.replaceState(null, '', clean)
  }

  const hash = window.location.hash.slice(1)
  if (hash && hash.includes('/')) {
    const parts = hash.split('/')
    const [id, code] = parts
    if (id && code) {
      joinChannelId.value = id
      joinPasscode.value = code
      mode.value = 'caller'
      // Parse TURN from 3rd segment if present
      if (parts[2]) {
        urlTurnServers = decodeTurn(parts[2])
        console.log('[App] Decoded TURN from URL:', JSON.stringify(urlTurnServers))
      }
      // Clear hash so it doesn't persist
      history.replaceState(null, '', window.location.pathname)
    }
  }
})

onUnmounted(() => {
  if (call) call.hangup()
  stopMedia()
})
</script>

<template>
  <h1>ZeroRTC Demo</h1>

  <!-- Mode selection -->
  <div v-if="status === 'idle' && !mode" class="mode-select">
    <button @click="selectMode('callee')">Create Call</button>
    <button @click="selectMode('caller')">Join Call</button>
  </div>

  <!-- Callee: create -->
  <div v-if="mode === 'callee' && status === 'idle'" class="panel">
    <h2>Create a new call</h2>
    <p style="color:#888; margin-bottom:1rem;">
      Create a channel and share the credentials with the caller.
    </p>

    <!-- Media permission -->
    <div v-if="!mediaReady" class="media-prompt">
      <p>Allow camera &amp; microphone access to start a call.</p>
      <button class="primary" @click="requestMedia">Allow Camera &amp; Mic</button>
      <div v-if="mediaError" class="status error" style="margin-top:0.5rem">{{ mediaError }}</div>
    </div>
    <div v-else class="media-preview">
      <video ref="localVideo" autoplay muted playsinline class="local-video"></video>

      <!-- TURN override -->
      <div class="turn-toggle" style="margin-top:0.75rem;">
        <a href="#" @click.prevent="showTurnOverride = !showTurnOverride" style="color:#4a9eff; font-size:0.85rem;">
          {{ showTurnOverride ? '▾ Hide' : '▸ Override' }} TURN Server
        </a>
      </div>
      <div v-if="showTurnOverride" class="turn-fields">
        <div class="field">
          <label>TURN URL</label>
          <input v-model="turnUrl" placeholder="turn:relay.example.com:3478" />
        </div>
        <div class="field">
          <label>Username</label>
          <input v-model="turnUsername" placeholder="(optional)" />
        </div>
        <div class="field">
          <label>Credential</label>
          <input v-model="turnCredential" placeholder="(optional)" type="password" />
        </div>
      </div>

      <button class="primary" @click="createCall" style="margin-top:0.75rem">Create Call</button>
    </div>
  </div>

  <!-- Callee: waiting -->
  <div v-if="mode === 'callee' && (status === 'waiting' || status === 'creating')" class="panel">
    <template v-if="!peerJoining">
      <h2>Waiting for caller...</h2>

      <div v-if="qrDataUrl" class="qr-section">
        <img :src="qrDataUrl" alt="QR Code" class="qr-code" />
        <p style="color:#888; font-size:0.85rem; margin-top:0.5rem;">Scan to join this call</p>
      </div>

      <div v-if="joinUrl" class="field" style="margin-top:1rem;">
        <label>Join Link</label>
        <div class="credential link-credential" @click="copyLink">{{ joinUrl }}</div>
        <p style="color:#666; font-size:0.75rem; margin-top:0.25rem;">Click to copy</p>
      </div>

      <div class="field">
        <label>Channel ID</label>
        <div class="credential">{{ channelId }}</div>
      </div>
      <div class="field">
        <label>Passcode</label>
        <div class="credential">{{ passcode }}</div>
      </div>
    </template>

    <div v-else class="connection-progress">
      <div class="progress-spinner"></div>
      <h2 class="progress-title">Someone is joining</h2>
      <p class="progress-subtitle">Establishing secure connection...</p>
      <div class="progress-bar"><div class="progress-fill indeterminate"></div></div>
    </div>
  </div>

  <!-- Caller: join -->
  <div v-if="mode === 'caller' && status === 'idle'" class="panel">
    <h2>Join a call</h2>
    <div class="field">
      <label>Channel ID</label>
      <input v-model="joinChannelId" placeholder="e.g. ABC123" />
    </div>
    <div class="field">
      <label>Passcode</label>
      <input v-model="joinPasscode" placeholder="e.g. 9876" />
    </div>

    <!-- Media permission -->
    <div v-if="!mediaReady" class="media-prompt">
      <p>Allow camera &amp; microphone access to join the call.</p>
      <button class="primary" @click="requestMedia">Allow Camera &amp; Mic</button>
      <div v-if="mediaError" class="status error" style="margin-top:0.5rem">{{ mediaError }}</div>
    </div>
    <div v-else class="media-preview">
      <video ref="localVideo" autoplay muted playsinline class="local-video"></video>
      <button
        class="primary"
        :disabled="!joinChannelId || !joinPasscode"
        @click="joinCall"
        style="margin-top:0.75rem"
      >
        Join Call
      </button>
    </div>
  </div>

  <!-- Caller: connecting -->
  <div v-if="mode === 'caller' && status === 'joining'" class="panel">
    <div class="connection-progress">
      <div class="progress-spinner"></div>
      <h2 class="progress-title">Connecting</h2>
      <p class="progress-subtitle">Joining channel...</p>
      <div class="progress-bar"><div class="progress-fill indeterminate"></div></div>
    </div>
  </div>

  <!-- Status -->
  <div
    v-if="statusText && !peerJoining && status !== 'joining' && status !== 'ended'"
    class="status"
    :class="{ connected: status === 'connected', error: status === 'error' }"
  >
    {{ statusText }}
  </div>
  <div v-if="errorText && status !== 'ended'" class="status error">{{ errorText }}</div>

  <!-- Video (placeholder for future media stream support) -->

  <!-- Video Streams -->
  <div v-if="status === 'connected'" class="panel video-panel">
    <h2>Video</h2>
    <div class="video-grid">
      <div class="video-box">
        <video ref="connectedLocalVideo" autoplay muted playsinline class="stream-video"></video>
        <span class="video-label">You</span>
      </div>
      <div class="video-box">
        <video ref="remoteVideo" autoplay playsinline class="stream-video"></video>
        <span class="video-label">Peer</span>
      </div>
    </div>
  </div>

  <!-- Chat -->
  <div v-if="status === 'connected'" class="panel">
    <h2>Chat</h2>
    <div class="messages">
      <div v-for="(m, i) in messages" :key="i" class="msg" :class="{ self: m.self }">
        {{ m.self ? 'You: ' : 'Peer: ' }}{{ m.text }}
      </div>
    </div>
    <div class="msg-input">
      <input v-model="msgInput" placeholder="Type a message..." @keyup.enter="sendMessage" />
      <button @click="sendMessage">Send</button>
    </div>
  </div>

  <!-- Call ended -->
  <div v-if="status === 'ended'" class="panel">
    <div class="call-ended">
      <div class="ended-icon">
        <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><polyline points="20 6 9 17 4 12"></polyline></svg>
      </div>
      <h2 class="ended-title">Call Ended</h2>
      <button v-if="mode === 'callee'" class="primary" style="margin-top:1.5rem" @click="startNew">New Call</button>
      <button v-if="mode === 'caller'" class="primary" style="margin-top:1.5rem" @click="closeTab">Close Tab</button>
    </div>
  </div>

  <!-- Hangup / Back -->
  <button
    v-if="status === 'waiting' || status === 'connected' || status === 'joining'"
    class="danger"
    @click="hangup"
  >
    Hang Up
  </button>
  <button
    v-if="mode && status === 'idle'"
    class="primary"
    style="margin-top:1rem"
    @click="mode = null"
  >
    ← Back
  </button>
</template>
