// DataChannel message chunking for messages exceeding ~16KB limit.
// Same protocol as P2PCF (see P2PCF_SYSTEMATIC_KNOWHOW.md §10).

const MAX_MESSAGE_LENGTH_BYTES = 16000
const CHUNK_HEADER_LENGTH_BYTES = 12 // 2+2+2+2+4
const CHUNK_MAGIC_WORD = 0x1FB9 // 8121 decimal
const CHUNK_MAX_LENGTH_BYTES = MAX_MESSAGE_LENGTH_BYTES - CHUNK_HEADER_LENGTH_BYTES

const SIGNAL_MESSAGE_HEADER_WORDS = [0x82ab, 0x81cd, 0x1295, 0xa1cb]

export class ChunkedChannel {
  constructor () {
    this.chunks = new Map()
    this._nextMsgId = 0
  }

  /**
   * Send data, chunking if necessary.
   * @param {RTCDataChannel} dc
   * @param {ArrayBuffer|Uint8Array|string} data
   */
  send (dc, data) {
    let buf
    if (typeof data === 'string') {
      buf = new TextEncoder().encode(data)
    } else if (data instanceof ArrayBuffer) {
      buf = new Uint8Array(data)
    } else {
      buf = data
    }

    // Check if chunking needed
    const needsChunking =
      buf.byteLength > MAX_MESSAGE_LENGTH_BYTES ||
      (buf.byteLength >= 2 && new DataView(buf.buffer, buf.byteOffset).getUint16(0, true) === CHUNK_MAGIC_WORD)

    if (!needsChunking) {
      dc.send(buf)
      return
    }

    const msgId = (this._nextMsgId++) & 0xFFFF
    const totalLen = buf.byteLength
    let offset = 0
    let chunkId = 0

    while (offset < totalLen) {
      const end = Math.min(offset + CHUNK_MAX_LENGTH_BYTES, totalLen)
      const chunkPayload = buf.slice(offset, end)
      const done = end >= totalLen ? 1 : 0

      const chunk = new ArrayBuffer(CHUNK_HEADER_LENGTH_BYTES + chunkPayload.byteLength)
      const view = new DataView(chunk)
      view.setUint16(0, CHUNK_MAGIC_WORD, true)
      view.setUint16(2, msgId, true)
      view.setUint16(4, chunkId, true)
      view.setUint16(6, done, true)
      view.setUint32(8, totalLen, true)
      new Uint8Array(chunk, CHUNK_HEADER_LENGTH_BYTES).set(chunkPayload)

      dc.send(chunk)
      offset = end
      chunkId++
    }
  }

  /**
   * Process incoming data. Handles chunked reassembly and signal detection.
   * @param {ArrayBuffer|Uint8Array} data
   * @param {function} onMessage - called with reassembled user data
   * @param {function} onSignal - called with parsed signal JSON (for re-negotiation)
   */
  onData (data, onMessage, onSignal) {
    let buf
    if (data instanceof ArrayBuffer) {
      buf = new Uint8Array(data)
    } else if (typeof data === 'string') {
      onMessage(data)
      return
    } else {
      buf = data
    }

    // Check for signal message header
    if (buf.byteLength >= 8) {
      const view = new DataView(buf.buffer, buf.byteOffset)
      if (
        view.getUint16(0, true) === SIGNAL_MESSAGE_HEADER_WORDS[0] &&
        view.getUint16(2, true) === SIGNAL_MESSAGE_HEADER_WORDS[1] &&
        view.getUint16(4, true) === SIGNAL_MESSAGE_HEADER_WORDS[2] &&
        view.getUint16(6, true) === SIGNAL_MESSAGE_HEADER_WORDS[3]
      ) {
        const json = new TextDecoder().decode(buf.slice(8))
        if (onSignal) onSignal(JSON.parse(json))
        return
      }
    }

    // Check for chunk magic
    if (buf.byteLength >= CHUNK_HEADER_LENGTH_BYTES) {
      const view = new DataView(buf.buffer, buf.byteOffset)
      if (view.getUint16(0, true) === CHUNK_MAGIC_WORD) {
        const msgId = view.getUint16(2, true)
        const chunkId = view.getUint16(4, true)
        const done = view.getUint16(6, true) !== 0
        const totalLen = view.getUint32(8, true)
        const payload = buf.slice(CHUNK_HEADER_LENGTH_BYTES)

        if (!this.chunks.has(msgId)) {
          this.chunks.set(msgId, new Uint8Array(totalLen))
        }

        const assembled = this.chunks.get(msgId)
        assembled.set(payload, chunkId * CHUNK_MAX_LENGTH_BYTES)

        if (done) {
          this.chunks.delete(msgId)
          onMessage(assembled)
        }
        return
      }
    }

    // Plain message
    onMessage(buf)
  }

  /**
   * Send signalling data over an established DataChannel.
   * @param {RTCDataChannel} dc
   * @param {object} signalData
   */
  sendSignal (dc, signalData) {
    const json = new TextEncoder().encode(JSON.stringify(signalData))
    const buf = new ArrayBuffer(8 + json.byteLength)
    const view = new DataView(buf)
    view.setUint16(0, SIGNAL_MESSAGE_HEADER_WORDS[0], true)
    view.setUint16(2, SIGNAL_MESSAGE_HEADER_WORDS[1], true)
    view.setUint16(4, SIGNAL_MESSAGE_HEADER_WORDS[2], true)
    view.setUint16(6, SIGNAL_MESSAGE_HEADER_WORDS[3], true)
    new Uint8Array(buf, 8).set(json)
    this.send(dc, new Uint8Array(buf))
  }
}
