<script setup>
import { ref, onMounted, onUnmounted } from 'vue'
import { ZeroRTC, generateQRPayload, parseQRPayload } from '../../client/src/index.js'
import QRCode from 'qrcode'

/* global __GIT_COMMIT__ */
const gitCommit = __GIT_COMMIT__

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
const turnEnabled = ref(false)
const turnUrl = ref('')
const turnUsername = ref('')
const turnCredential = ref('')

// Channel config (require / additional)
const cfgBandwidth = ref('2M')
const cfgBandwidthEnabled = ref(true)

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

// Video stats
const localRes = ref('')
const remoteRes = ref('')
const sendBitrate = ref('')
const recvBitrate = ref('')
let statsTimer = null
let prevBytesSent = 0
let prevBytesRecv = 0
let prevStatsTime = 0



// localStorage helpers for TURN
function saveTurnToStorage () {
  const obj = {}
  obj.e = turnEnabled.value
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
    if (obj.e !== undefined) turnEnabled.value = obj.e
    if (obj.u) turnUrl.value = obj.u
    if (obj.n) turnUsername.value = obj.n
    if (obj.c) turnCredential.value = obj.c
  } catch {}
}

async function requestMedia (width, height) {
  try {
    mediaError.value = ''
    const videoConstraints = (width && height)
      ? { width: { ideal: width }, height: { ideal: height } }
      : true
    const stream = await navigator.mediaDevices.getUserMedia({ video: videoConstraints, audio: true })
    localStream.value = stream
    mediaReady.value = true
    // Attach to video element after next tick
    setTimeout(() => {
      if (localVideo.value) {
        localVideo.value.srcObject = stream
      }
    }, 50)
    const vt = stream.getVideoTracks()[0]
    if (vt) {
      const s = vt.getSettings()
      console.log(`[App] Acquired video: ${s.width}×${s.height}`)
    }
    return stream
  } catch (err) {
    mediaError.value = err.name === 'NotAllowedError'
      ? 'Camera/microphone permission denied. Please allow access and try again.'
      : `Media error: ${err.message}`
    mediaReady.value = false
    throw err
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
    // Apply bitrate limit from require config
    applyBitrateLimit()
    startStats()
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
    saveTurnToStorage()

    // Build require / additional config for the channel
    const bandwidthMap = { '1M': 1000000, '2M': 2000000, '3M': 3000000, '4M': 4000000, '5M': 5000000, '10M': 10000000 }
    const requireConfig = {}
    if (cfgBandwidthEnabled.value) {
      requireConfig.max_bitrate = bandwidthMap[cfgBandwidth.value] || 2000000
    }
    const additionalConfig = {}
    const useTurn = turnEnabled.value && turnUrl.value
    if (useTurn) {
      additionalConfig.turn = { urls: turnUrl.value }
      if (turnUsername.value) additionalConfig.turn.username = turnUsername.value
      if (turnCredential.value) additionalConfig.turn.credential = turnCredential.value
    }

    // 1. Create channel first to get URL/QR
    call = new ZeroRTC({
      workerUrl: WORKER_URL,
      turnIceServers: useTurn ? [{
        urls: turnUrl.value,
        username: turnUsername.value || undefined,
        credential: turnCredential.value || undefined
      }] : undefined
    })
    wireEvents()

    const creds = await call.create({
      require: Object.keys(requireConfig).length ? requireConfig : undefined,
      additional: Object.keys(additionalConfig).length ? additionalConfig : undefined
    })
    channelId.value = creds.channelId
    passcode.value = creds.passcode

    // Generate join URL and QR code
    const hash = `${creds.channelId}/${creds.passcode}`
    const url = `${window.location.origin}${window.location.pathname}#${hash}`
    joinUrl.value = url
    qrDataUrl.value = await QRCode.toDataURL(url, { width: 256, margin: 2, color: { dark: '#000', light: '#fff' } })

    status.value = 'waiting'
    statusText.value = 'Scan the QR code or share the link below'

    // 2. Request media
    await requestMedia()
    call.localStream = localStream.value

    // 3. listen() — generates SDP with the correct resolution track
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
      turnIceServers: (turnEnabled.value && turnUrl.value) ? [{
        urls: turnUrl.value,
        username: turnUsername.value || undefined,
        credential: turnCredential.value || undefined
      }] : undefined
    })
    wireEvents()

    // 1. Join to get callee signal + require config (no WebRTC yet)
    const joinResult = await call.signaller.join(joinChannelId.value, joinPasscode.value)
    call.channelId = joinChannelId.value
    call.passcode = joinPasscode.value
    call.role = 'caller'
    call.require = joinResult.require ?? null
    call.additional = joinResult.additional ?? null

    // Apply TURN from channel config if no local TURN was provided
    if (call.additional?.turn && !call.turnIceServers) {
      call.turnIceServers = [call.additional.turn]
    }

    // 2. Request media
    await requestMedia()
    call.localStream = localStream.value

    // 3. Do the WebRTC exchange with the correct resolution track
    await call._doJoin(joinResult)
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
  stopStats()
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

function formatBitrate (bps) {
  if (bps >= 1000000) return (bps / 1000000).toFixed(1) + ' Mbps'
  if (bps >= 1000) return (bps / 1000).toFixed(0) + ' kbps'
  return bps + ' bps'
}

function startStats () {
  prevBytesSent = 0
  prevBytesRecv = 0
  prevStatsTime = Date.now()
  statsTimer = setInterval(async () => {
    // Local resolution from video track settings
    if (localStream.value) {
      const vt = localStream.value.getVideoTracks()[0]
      if (vt) {
        const s = vt.getSettings()
        if (s.width && s.height) localRes.value = `${s.width}×${s.height}`
      }
    }
    // Remote resolution from video element
    if (remoteVideo.value) {
      const w = remoteVideo.value.videoWidth
      const h = remoteVideo.value.videoHeight
      if (w && h) remoteRes.value = `${w}×${h}`
    }
    // Bitrate from RTCPeerConnection stats
    if (!call || !call.pc) return
    try {
      const stats = await call.pc.getStats()
      const now = Date.now()
      const elapsed = (now - prevStatsTime) / 1000
      if (elapsed <= 0) return
      let totalSent = 0
      let totalRecv = 0
      stats.forEach(report => {
        const isVideo = report.kind === 'video' || report.mediaType === 'video'
        if (report.type === 'outbound-rtp' && isVideo) {
          totalSent += report.bytesSent || 0
        }
        if (report.type === 'inbound-rtp' && isVideo) {
          totalRecv += report.bytesReceived || 0
        }
      })
      if (prevBytesSent > 0) {
        sendBitrate.value = formatBitrate(((totalSent - prevBytesSent) * 8) / elapsed)
      }
      if (prevBytesRecv > 0) {
        recvBitrate.value = formatBitrate(((totalRecv - prevBytesRecv) * 8) / elapsed)
      }
      prevBytesSent = totalSent
      prevBytesRecv = totalRecv
      prevStatsTime = now
    } catch {}
  }, 1000)
}

function stopStats () {
  if (statsTimer) { clearInterval(statsTimer); statsTimer = null }
  localRes.value = ''
  remoteRes.value = ''
  sendBitrate.value = ''
  recvBitrate.value = ''
}

function toggleFullscreen (el) {
  if (document.fullscreenElement === el) {
    document.exitFullscreen()
  } else {
    el.requestFullscreen().catch(() => {})
  }
}

async function applyBitrateLimit () {
  if (!call || !call.pc) return
  // Callee: use local config; Caller: use channel require config
  const bandwidthMap = { '1M': 1000000, '2M': 2000000, '3M': 3000000, '4M': 4000000, '5M': 5000000, '10M': 10000000 }
  const localBitrate = cfgBandwidthEnabled.value
    ? bandwidthMap[cfgBandwidth.value] : null
  const bitrate = call.require?.max_bitrate || localBitrate
  if (!bitrate) return
  try {
    for (const sender of call.pc.getSenders()) {
      if (sender.track?.kind !== 'video') continue
      const params = sender.getParameters()
      if (!params.encodings?.length) continue
      params.encodings[0].maxBitrate = bitrate
      await sender.setParameters(params)
      console.log(`[App] Set maxBitrate: ${bitrate}`)
    }
  } catch (err) {
    console.warn('[App] Failed to set bitrate limit:', err)
  }
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

  const hash = window.location.hash.slice(1)
  if (hash && hash.includes('/')) {
    const parts = hash.split('/')
    const [id, code] = parts
    if (id && code) {
      joinChannelId.value = id
      joinPasscode.value = code
      mode.value = 'caller'
      // Clear hash so it doesn't persist
      history.replaceState(null, '', window.location.pathname)
    }
  }
})

function onBeforeUnload (e) {
  if (call && ['creating', 'waiting', 'joining', 'connected'].includes(status.value)) {
    e.preventDefault()
    e.returnValue = ''
  }
}

window.addEventListener('beforeunload', onBeforeUnload)

onUnmounted(() => {
  window.removeEventListener('beforeunload', onBeforeUnload)
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
    <a href="#" @click.prevent="mode = null" class="back-link">← Back</a>
    <h2>Create a new call</h2>
    <p style="color:#888; margin-bottom:1rem;">
      Create a channel and share the credentials with the caller.
    </p>

    <!-- Settings -->
    <details class="settings-panel">
      <summary>Settings</summary>

      <div class="settings-grid">
        <label class="setting-row">
          <input type="checkbox" v-model="cfgBandwidthEnabled" />
          <span class="setting-label">Max Bandwidth</span>
          <select v-model="cfgBandwidth" :disabled="!cfgBandwidthEnabled">
            <option>1M</option>
            <option>2M</option>
            <option>3M</option>
            <option>4M</option>
            <option>5M</option>
            <option>10M</option>
          </select>
        </label>

        <label class="setting-row">
          <input type="checkbox" v-model="turnEnabled" />
          <span class="setting-label">TURN Server</span>
          <input v-model="turnUrl" placeholder="turn:relay.example.com:3478" :disabled="!turnEnabled" class="setting-input" />
        </label>

        <div v-if="turnEnabled" class="setting-sub">
          <label class="setting-row">
            <span class="setting-label">Username</span>
            <input v-model="turnUsername" placeholder="(optional)" class="setting-input" />
          </label>
          <label class="setting-row">
            <span class="setting-label">Credential</span>
            <input v-model="turnCredential" placeholder="(optional)" type="password" class="setting-input" />
          </label>
        </div>
      </div>
    </details>

    <div class="call-summary">
      <span v-if="turnEnabled && turnUrl">TURN: <b>{{ turnUrl }}</b></span>
      <span v-else>STUN only</span>
      <span v-if="cfgBandwidthEnabled"> · {{ cfgBandwidth }}</span>
    </div>

    <div v-if="mediaError" class="status error" style="margin-top:0.5rem">{{ mediaError }}</div>
    <button class="primary" @click="createCall" style="margin-top:0.75rem">Create Call</button>
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

      <div v-if="mediaReady" class="media-preview" style="margin-top:1rem">
        <video ref="localVideo" autoplay muted playsinline class="local-video"></video>
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
    <a href="#" @click.prevent="mode = null" class="back-link">← Back</a>
    <h2>Join a call</h2>
    <div class="field">
      <label>Channel ID</label>
      <input v-model="joinChannelId" placeholder="e.g. ABC123" />
    </div>
    <div class="field">
      <label>Passcode</label>
      <input v-model="joinPasscode" placeholder="e.g. 9876" />
    </div>
    <div v-if="mediaError" class="status error" style="margin-top:0.5rem">{{ mediaError }}</div>
    <button
      class="primary"
      :disabled="!joinChannelId || !joinPasscode"
      @click="joinCall"
      style="margin-top:0.75rem"
    >
      Join Call
    </button>
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
      <div class="video-box" @click="toggleFullscreen($event.currentTarget)">
        <video ref="connectedLocalVideo" autoplay muted playsinline class="stream-video"></video>
        <div class="video-overlay">
          <span class="video-label">You</span>
          <span v-if="localRes" class="stat">{{ localRes }}</span>
          <span v-if="sendBitrate" class="stat">↑ {{ sendBitrate }}</span>
        </div>
      </div>
      <div class="video-box" @click="toggleFullscreen($event.currentTarget)">
        <video ref="remoteVideo" autoplay playsinline class="stream-video"></video>
        <div class="video-overlay">
          <span class="video-label">Peer</span>
          <span v-if="remoteRes" class="stat">{{ remoteRes }}</span>
          <span v-if="recvBitrate" class="stat">↓ {{ recvBitrate }}</span>
        </div>
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

  <div class="git-commit">{{ gitCommit }}</div>
</template>
