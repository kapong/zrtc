import { createSignaller } from './signalling.js'
import { generateDtlsCert, prepareOffer, prepareAnswer, finalizeOffer, finalizeAnswer } from './webrtc.js'
import { ChunkedChannel } from './chunking.js'

// Minimal browser-compatible EventEmitter (avoids Node.js 'events' polyfill)
class EventEmitter {
  constructor () { this._listeners = {} }
  on (e, fn) { (this._listeners[e] = this._listeners[e] || []).push(fn); return this }
  off (e, fn) { if (this._listeners[e]) this._listeners[e] = this._listeners[e].filter(f => f !== fn); return this }
  emit (e, ...args) { (this._listeners[e] || []).forEach(fn => fn(...args)) }
}

const DEFAULT_STUN = [{ urls: 'stun:stun.l.google.com:19302' }]
const HANGUP_MARKER = new Uint8Array([0xFF, 0x00])

export class ZeroRTC extends EventEmitter {
  constructor (options = {}) {
    super()
    this.workerUrl = options.workerUrl
    this.stunIceServers = options.stunIceServers
    this.turnIceServers = options.turnIceServers
    this.localStream = options.localStream || null
    this.fastPollingRateMs = options.fastPollingRateMs || 500
    this.slowPollingRateMs = options.slowPollingRateMs || 2000

    this.channelId = null
    this.passcode = null
    this.role = null
    this.pc = null
    this.dataChannel = null
    this.chunked = new ChunkedChannel()
    this.signaller = createSignaller({ workerUrl: this.workerUrl })
    this.destroyed = false
    this._pollTimer = null
    this.require = null
    this.additional = null
  }

  /** Create a new channel (callee side). Returns { channelId, passcode }. */
  async create (options = {}) {
    const { require, additional, ...rest } = options
    const body = { ...rest }
    if (require !== undefined) body.require = require
    if (additional !== undefined) body.additional = additional
    const result = await this.signaller.createChannel(body)
    this.channelId = result.channel_id
    this.passcode = result.passcode
    this.role = 'callee'
    return { channelId: this.channelId, passcode: this.passcode }
  }

  /** Post callee signal and poll until caller joins. Emits 'connected'. */
  async listen () {
    if (!this.channelId || this.role !== 'callee') {
      throw new Error('Must call create() first')
    }

    const iceServers = this._iceServers()
    console.log('[ZeroRTC] listen() iceServers:', JSON.stringify(iceServers))
    const dtlsCert = await generateDtlsCert()

    // Create the actual PC + offer + gather candidates (PC stays open)
    const { pc, dataChannel, signal } = await prepareOffer(iceServers, dtlsCert, this.localStream)

    // Set ontrack early — callee receives tracks when finalizeOffer calls setRemoteDescription
    const onTrack = (e) => {
      console.log('[ZeroRTC] ontrack:', e.track.kind)
      if (e.streams && e.streams[0]) this.emit('stream', e.streams[0])
    }

    await this.signaller.listen(this.channelId, this.passcode, signal)
    console.log('[ZeroRTC] listen() signal posted, polling for caller...')

    await this._pollForCaller(pc, dataChannel, onTrack)
  }

  /** Join an existing channel (caller side). Emits 'connected'. */
  async join (channelId, passcode) {
    this.channelId = channelId
    this.passcode = passcode
    this.role = 'caller'

    const dtlsCert = await generateDtlsCert()

    // Join without signal — just get callee's signal
    const result = await this.signaller.join(this.channelId, this.passcode)

    // Expose config objects from join response
    this.require = result.require ?? null
    this.additional = result.additional ?? null

    // Apply TURN from channel config if no local TURN was provided
    if (this.additional?.turn && !this.turnIceServers) {
      this.turnIceServers = [this.additional.turn]
    }

    const iceServers = this._iceServers()
    console.log('[ZeroRTC] join() iceServers:', JSON.stringify(iceServers))

    console.log('[ZeroRTC] join() got callee signal:', {
      ufrag: result.callee_signal?.ice_ufrag,
      candidates: result.callee_signal?.candidates?.length
    })

    // Create answerer PC using callee's signal as remote offer (PC stays open)
    const onTrack = (e) => {
      console.log('[ZeroRTC] ontrack:', e.track.kind)
      if (e.streams && e.streams[0]) this.emit('stream', e.streams[0])
    }
    const { pc, signal } = await prepareAnswer(result.callee_signal, iceServers, dtlsCert, this.localStream, onTrack)

    // Push our signal to the worker for the callee to poll
    await this.signaller.pushSignal(this.channelId, this.passcode, signal)
    console.log('[ZeroRTC] join() signal pushed, waiting for connection...')

    // Wait for the datachannel from the callee side
    const conn = await finalizeAnswer(pc)
    this._wire(conn.pc, conn.dataChannel)
  }

  /** Send data to the peer (auto-chunks if needed). */
  send (data) {
    if (!this.dataChannel || this.dataChannel.readyState !== 'open') {
      throw new Error('DataChannel not open')
    }
    this.chunked.send(this.dataChannel, data)
  }

  /** Hang up: notify peer, close connection, tell worker. */
  async hangup () {
    this.destroyed = true
    if (this._pollTimer) clearTimeout(this._pollTimer)

    if (this.dataChannel && this.dataChannel.readyState === 'open') {
      try { this.dataChannel.send(HANGUP_MARKER) } catch (_) {}
    }

    if (this.pc) {
      this.pc.close()
      this.pc = null
      this.dataChannel = null
    }

    if (this.channelId && this.passcode) {
      try { await this.signaller.hangup(this.channelId, this.passcode, this.role) } catch (_) {}
    }

    this.emit('hangup')
  }

  // --- internal ---

  _iceServers () {
    const servers = [...(this.stunIceServers || DEFAULT_STUN)]
    if (this.turnIceServers) {
      servers.push(...this.turnIceServers)
    }
    return servers
  }

  async _pollForCaller (pc, dataChannel, onTrack) {
    return new Promise((resolve, reject) => {
      let rate = this.fastPollingRateMs

      const doPoll = async () => {
        if (this.destroyed) return reject(new Error('Destroyed'))
        try {
          const result = await this.signaller.poll(this.channelId, this.passcode)
          if (result.state === 'LOCKED' && result.caller_signal) {
            this.emit('joining')

            // Expose config objects from LOCKED poll response
            this.require = result.require ?? this.require
            this.additional = result.additional ?? this.additional

            console.log('[ZeroRTC] poll() got caller signal:', {
              ufrag: result.caller_signal?.ice_ufrag,
              candidates: result.caller_signal?.candidates?.length
            })
            // Finalize the existing PC with the caller's answer signal
            const conn = await finalizeOffer(pc, dataChannel, result.caller_signal, onTrack)
            this._wire(conn.pc, conn.dataChannel)
            return resolve()
          }
          if (result.state === 'LOCKED') {
            this.emit('joining')
          }
          rate = this.slowPollingRateMs
        } catch (err) {
          this.emit('error', err)
          return reject(err)
        }
        this._pollTimer = setTimeout(doPoll, rate)
      }

      doPoll()
    })
  }

  _wire (pc, dataChannel) {
    this.pc = pc
    this.dataChannel = dataChannel
    dataChannel.binaryType = 'arraybuffer'

    // Handle remote media tracks
    pc.ontrack = (e) => {
      console.log('[ZeroRTC] ontrack:', e.track.kind)
      if (e.streams && e.streams[0]) {
        this.emit('stream', e.streams[0])
      }
    }

    dataChannel.onmessage = (event) => {
      const d = event.data
      // Detect hangup marker
      if (d instanceof ArrayBuffer && d.byteLength === 2) {
        const v = new Uint8Array(d)
        if (v[0] === 0xFF && v[1] === 0x00) {
          this.emit('hangup')
          return
        }
      }
      this.chunked.onData(d, (msg) => this.emit('data', msg), null)
    }

    dataChannel.onclose = () => {
      if (!this.destroyed) this.emit('disconnected')
    }

    pc.oniceconnectionstatechange = () => {
      const s = pc.iceConnectionState
      if (s === 'disconnected') this.emit('disconnected')
      else if (s === 'failed') this.emit('error', new Error('ICE connection failed'))
    }

    this.emit('connected')
  }
}
