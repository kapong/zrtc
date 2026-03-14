// QR payload encode/decode helpers.
// These are pure utility functions — no QR rendering/scanning.
// Actual QR image generation and camera scanning is left to the UI layer.

/**
 * Generate a QR-friendly payload string from channel credentials.
 * @param {string} workerUrl - The worker base URL
 * @param {string} channelId - The channel ID
 * @param {string} passcode - The passcode
 * @returns {string} URL-formatted payload for QR encoding
 */
export function generateQRPayload (workerUrl, channelId, passcode) {
  // Use URL fragment (after #) so passcode is never sent to server in a GET request
  return `${workerUrl}/join#${channelId}:${passcode}`
}

/**
 * Parse a scanned QR payload back into components.
 * @param {string} payload - The scanned QR string
 * @returns {{ workerUrl: string, channelId: string, passcode: string }}
 */
export function parseQRPayload (payload) {
  const hashIndex = payload.indexOf('#')
  if (hashIndex === -1) throw new Error('Invalid QR payload: missing #')

  const workerUrl = payload.substring(0, hashIndex).replace(/\/join$/, '')
  const fragment = payload.substring(hashIndex + 1)
  const colonIndex = fragment.indexOf(':')
  if (colonIndex === -1) throw new Error('Invalid QR payload: missing :')

  return {
    workerUrl,
    channelId: fragment.substring(0, colonIndex),
    passcode: fragment.substring(colonIndex + 1)
  }
}
