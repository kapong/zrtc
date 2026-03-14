// Client unit tests — chunking logic.
// Run: node tests/test-chunking.js
import { strict as assert } from 'node:assert'

// Import chunking source directly (ESM)
const { ChunkedChannel } = await import('../client/src/chunking.js')

let pass = 0
let fail = 0

function test (name, fn) {
  try {
    fn()
    console.log(`  \x1b[32m✓\x1b[0m ${name}`)
    pass++
  } catch (e) {
    console.log(`  \x1b[31m✗\x1b[0m ${name}`)
    console.log(`    ${e.message}`)
    fail++
  }
}

// Mock DataChannel
function mockDC () {
  const sent = []
  return {
    sent,
    send (data) { sent.push(data instanceof ArrayBuffer ? new Uint8Array(data) : data) },
    readyState: 'open'
  }
}

console.log('\n\x1b[1mChunkedChannel tests\x1b[0m\n')

// ── Small message (no chunking) ──
test('small message sent without chunking', () => {
  const ch = new ChunkedChannel()
  const dc = mockDC()
  const data = new Uint8Array([1, 2, 3, 4, 5])
  ch.send(dc, data)
  assert.equal(dc.sent.length, 1)
  assert.deepEqual(dc.sent[0], data)
})

// ── String message ──
test('string message encoded to Uint8Array', () => {
  const ch = new ChunkedChannel()
  const dc = mockDC()
  ch.send(dc, 'hello')
  assert.equal(dc.sent.length, 1)
  const decoded = new TextDecoder().decode(dc.sent[0])
  assert.equal(decoded, 'hello')
})

// ── Large message triggers chunking ──
test('large message is chunked', () => {
  const ch = new ChunkedChannel()
  const dc = mockDC()
  const data = new Uint8Array(20000) // > 16000 threshold
  data.fill(42)
  ch.send(dc, data)
  assert.ok(dc.sent.length > 1, `expected multiple chunks, got ${dc.sent.length}`)
  // Each chunk should start with magic word 0x1FB9
  for (const chunk of dc.sent) {
    const view = new DataView(chunk.buffer, chunk.byteOffset)
    assert.equal(view.getUint16(0, true), 0x1FB9, 'chunk magic word')
  }
})

// ── Chunk reassembly ──
test('chunked message reassembles correctly', () => {
  const sender = new ChunkedChannel()
  const receiver = new ChunkedChannel()
  const dc = mockDC()

  // Send large data
  const original = new Uint8Array(20000)
  for (let i = 0; i < original.length; i++) original[i] = i % 256
  sender.send(dc, original)

  // Receive all chunks
  let reassembled = null
  for (const chunk of dc.sent) {
    receiver.onData(chunk, (msg) => { reassembled = msg }, null)
  }

  assert.ok(reassembled, 'message should have been reassembled')
  assert.equal(reassembled.length, original.length)
  assert.deepEqual(reassembled, original)
})

// ── Signal message ──
test('signal message round-trip', () => {
  const ch = new ChunkedChannel()
  const dc = mockDC()
  const signalData = { type: 'offer', sdp: 'test-sdp' }
  ch.sendSignal(dc, signalData)

  let received = null
  const receiver = new ChunkedChannel()
  // sendSignal wraps with send(), so pick the final chunk(s)
  for (const chunk of dc.sent) {
    receiver.onData(chunk, () => {}, (sig) => { received = sig })
  }

  assert.ok(received, 'signal should have been received')
  assert.equal(received.type, 'offer')
  assert.equal(received.sdp, 'test-sdp')
})

// ── Plain message passthrough in onData ──
test('plain binary message passed through', () => {
  const ch = new ChunkedChannel()
  const data = new Uint8Array([10, 20, 30])
  let received = null
  ch.onData(data, (msg) => { received = msg }, null)
  assert.deepEqual(received, data)
})

// ── String message passthrough in onData ──
test('string message passed through', () => {
  const ch = new ChunkedChannel()
  let received = null
  ch.onData('hello world', (msg) => { received = msg }, null)
  assert.equal(received, 'hello world')
})

// ── Data starting with magic word is force-chunked ──
test('data starting with magic word is force-chunked', () => {
  const ch = new ChunkedChannel()
  const dc = mockDC()
  // Create a small message that starts with 0x1FB9 (magic word)
  const data = new Uint8Array(10)
  const view = new DataView(data.buffer)
  view.setUint16(0, 0x1FB9, true)
  ch.send(dc, data)
  // Should be chunked even though < 16000 bytes
  assert.ok(dc.sent.length >= 1)
  const chunkView = new DataView(dc.sent[0].buffer, dc.sent[0].byteOffset)
  assert.equal(chunkView.getUint16(0, true), 0x1FB9, 'first chunk has magic')
})

// ── Summary ──
console.log('')
console.log('\x1b[1m━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\x1b[0m')
if (fail === 0) {
  console.log(`\x1b[32mAll ${pass + fail} tests passed ✓\x1b[0m`)
} else {
  console.log(`\x1b[31m${fail}/${pass + fail} tests failed\x1b[0m`)
}
console.log('\x1b[1m━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\x1b[0m')
process.exit(fail)
