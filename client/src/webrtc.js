// WebRTC peer connection management.
// Full SDP exchange — supports DataChannel + audio/video media tracks.
// Same-PC architecture: the PC used for gathering IS the connection PC.

const DEFAULT_STUN = [
  { urls: 'stun:stun.l.google.com:19302' },
  { urls: 'stun:global.stun.twilio.com:3478' }
]

const CONNECTION_TIMEOUT_MS = 30000
const ICE_GATHER_TIMEOUT_MS = 8000

/**
 * Generate a DTLS certificate (ECDSA P-256).
 */
export async function generateDtlsCert () {
  return RTCPeerConnection.generateCertificate({
    name: 'ECDSA',
    namedCurve: 'P-256'
  })
}

/**
 * Wait for ICE gathering to complete (or timeout).
 */
function waitForIceGathering (pc) {
  if (pc.iceGatheringState === 'complete') return Promise.resolve()
  return new Promise((resolve) => {
    const timeout = setTimeout(() => {
      console.log('[ZeroRTC] ICE gathering timeout, state:', pc.iceGatheringState)
      resolve()
    }, ICE_GATHER_TIMEOUT_MS)

    pc.onicegatheringstatechange = () => {
      console.log('[ZeroRTC] iceGatheringState:', pc.iceGatheringState)
      if (pc.iceGatheringState === 'complete') {
        clearTimeout(timeout)
        resolve()
      }
    }

    // Also log individual candidates for debugging
    const prevHandler = pc.onicecandidate
    pc.onicecandidate = (e) => {
      if (prevHandler) prevHandler(e)
      if (e.candidate) {
        console.log('[ZeroRTC] ICE candidate:', e.candidate.type || '?', e.candidate.candidate)
      }
    }
  })
}

/**
 * Callee (initiator): Create PeerConnection + offer + gather.
 * @param {object[]} iceServers
 * @param {RTCCertificate} dtlsCert
 * @param {MediaStream} [localStream] - local media to send
 * Returns { pc, dataChannel, signal } — PC stays open.
 */
export async function prepareOffer (iceServers = DEFAULT_STUN, dtlsCert, localStream) {
  console.log('[ZeroRTC] prepareOffer iceServers:', JSON.stringify(iceServers))

  const pc = new RTCPeerConnection({
    iceServers: iceServers.map(s => (typeof s === 'string' ? { urls: s } : s)),
    certificates: dtlsCert ? [dtlsCert] : undefined
  })

  // Add media tracks before creating offer so SDP includes them
  if (localStream) {
    for (const track of localStream.getTracks()) {
      pc.addTrack(track, localStream)
    }
    console.log('[ZeroRTC] prepareOffer added', localStream.getTracks().length, 'tracks')
  }

  const dataChannel = pc.createDataChannel('zrtc', { ordered: true })

  const offer = await pc.createOffer()
  await pc.setLocalDescription(offer)

  await waitForIceGathering(pc)

  // Full SDP with candidates already embedded by the browser
  const signal = { sdp: pc.localDescription.sdp, type: 'offer' }
  console.log('[ZeroRTC] prepareOffer SDP length:', signal.sdp.length)

  return { pc, dataChannel, signal }
}

/**
 * Caller (answerer): Create PeerConnection, set remote offer, create answer, gather.
 * @param {object} remoteSignal - { sdp, type:'offer' }
 * @param {object[]} iceServers
 * @param {RTCCertificate} dtlsCert
 * @param {MediaStream} [localStream] - local media to send
 * Returns { pc, signal } — PC stays open.
 */
export async function prepareAnswer (remoteSignal, iceServers = DEFAULT_STUN, dtlsCert, localStream, onTrack) {
  console.log('[ZeroRTC] prepareAnswer iceServers:', JSON.stringify(iceServers))
  console.log('[ZeroRTC] prepareAnswer remote SDP length:', remoteSignal.sdp?.length)

  const pc = new RTCPeerConnection({
    iceServers: iceServers.map(s => (typeof s === 'string' ? { urls: s } : s)),
    certificates: dtlsCert ? [dtlsCert] : undefined
  })

  // Add media tracks before creating answer
  if (localStream) {
    for (const track of localStream.getTracks()) {
      pc.addTrack(track, localStream)
    }
    console.log('[ZeroRTC] prepareAnswer added', localStream.getTracks().length, 'tracks')
  }

  // Set ontrack BEFORE setRemoteDescription so we catch track events
  if (onTrack) pc.ontrack = onTrack

  await pc.setRemoteDescription({ type: remoteSignal.type, sdp: remoteSignal.sdp })
  const answer = await pc.createAnswer()
  await pc.setLocalDescription(answer)

  await waitForIceGathering(pc)

  const signal = { sdp: pc.localDescription.sdp, type: 'answer' }
  console.log('[ZeroRTC] prepareAnswer SDP length:', signal.sdp.length)

  return { pc, signal }
}

/**
 * Callee finalization: set remote answer on existing PC.
 * Returns promise that resolves with { pc, dataChannel } when DataChannel opens.
 */
export function finalizeOffer (pc, dataChannel, remoteSignal, onTrack) {
  console.log('[ZeroRTC] finalizeOffer remote SDP length:', remoteSignal.sdp?.length)

  // Set ontrack BEFORE setRemoteDescription so we catch track events
  if (onTrack) pc.ontrack = onTrack

  return new Promise(async (resolve, reject) => {
    try {
      await pc.setRemoteDescription({ type: remoteSignal.type, sdp: remoteSignal.sdp })
    } catch (e) {
      console.error('[ZeroRTC] setRemoteDescription failed:', e.message)
      return reject(e)
    }

    waitForConnection(pc, dataChannel, resolve, reject)
  })
}

/**
 * Caller finalization: remote description already set during prepareAnswer.
 * Wait for DataChannel from the remote side.
 */
export function finalizeAnswer (pc) {
  console.log('[ZeroRTC] finalizeAnswer — waiting for datachannel...')

  return new Promise((resolve, reject) => {
    let dataChannel = null

    pc.ondatachannel = (e) => {
      console.log('[ZeroRTC] ondatachannel received:', e.channel.label)
      dataChannel = e.channel
      if (dataChannel.readyState === 'open') {
        onOpen()
      } else {
        dataChannel.onopen = onOpen
      }
    }

    const timeout = setTimeout(() => {
      console.error('[ZeroRTC] finalizeAnswer timeout! iceState:', pc.iceConnectionState,
        'signalingState:', pc.signalingState)
      pc.close()
      reject(new Error('Connection timeout'))
    }, CONNECTION_TIMEOUT_MS)

    function onOpen () {
      console.log('[ZeroRTC] DataChannel OPEN (caller)!')
      clearTimeout(timeout)
      resolve({ pc, dataChannel })
    }

    pc.oniceconnectionstatechange = () => {
      console.log('[ZeroRTC] caller iceConnectionState:', pc.iceConnectionState)
      if (pc.iceConnectionState === 'failed') {
        clearTimeout(timeout)
        pc.close()
        reject(new Error('ICE connection failed'))
      }
    }

    pc.onconnectionstatechange = () => {
      console.log('[ZeroRTC] caller connectionState:', pc.connectionState)
    }
  })
}

/**
 * Wait for DataChannel to open on an existing PC (callee side).
 */
function waitForConnection (pc, dataChannel, resolve, reject) {
  const timeout = setTimeout(() => {
    console.error('[ZeroRTC] waitForConnection timeout! iceState:', pc.iceConnectionState,
      'signalingState:', pc.signalingState,
      'dcState:', dataChannel?.readyState)
    pc.close()
    reject(new Error('Connection timeout'))
  }, CONNECTION_TIMEOUT_MS)

  function onOpen () {
    console.log('[ZeroRTC] DataChannel OPEN (callee)!')
    clearTimeout(timeout)
    resolve({ pc, dataChannel })
  }

  if (dataChannel.readyState === 'open') {
    onOpen()
    return
  }

  dataChannel.onopen = onOpen

  pc.oniceconnectionstatechange = () => {
    console.log('[ZeroRTC] callee iceConnectionState:', pc.iceConnectionState)
    if (pc.iceConnectionState === 'failed') {
      clearTimeout(timeout)
      pc.close()
      reject(new Error('ICE connection failed'))
    }
  }

  pc.onconnectionstatechange = () => {
    console.log('[ZeroRTC] callee connectionState:', pc.connectionState)
  }
}
