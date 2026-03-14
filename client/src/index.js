/**
 * ZeroRTC — Anonymous one-time P2P WebRTC connections.
 *
 * Usage:
 *   import { ZeroRTC } from 'zrtc'
 *
 *   // Callee
 *   const call = new ZeroRTC({ workerUrl: 'https://myworker.dev' })
 *   const { channelId, passcode } = await call.create()
 *   await call.listen()
 *   call.on('connected', (peer) => { ... })
 *
 *   // Caller
 *   const call = new ZeroRTC({ workerUrl: 'https://myworker.dev' })
 *   await call.join(channelId, passcode)
 */

export { ZeroRTC } from './zrtc.js'
export { generateQRPayload, parseQRPayload } from './qr.js'
