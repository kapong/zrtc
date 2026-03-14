// HTTP polling signalling — communicates with the worker API.

export function createSignaller ({ workerUrl }) {
  async function post (path, body) {
    const res = await fetch(`${workerUrl}${path}`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body)
    })
    const data = await res.json()
    if (!res.ok) {
      const err = new Error(data.message || data.error || `HTTP ${res.status}`)
      err.code = data.error
      err.status = res.status
      throw err
    }
    return data
  }

  return {
    async createChannel (options = {}) {
      return post('/new', options)
    },

    async listen (channelId, passcode, signal) {
      return post('/listen', {
        channel_id: channelId,
        passcode,
        role: 'callee',
        signal
      })
    },

    async join (channelId, passcode) {
      return post('/join', {
        channel_id: channelId,
        passcode,
        role: 'caller'
      })
    },

    async poll (channelId, passcode) {
      return post('/poll', {
        channel_id: channelId,
        passcode,
        role: 'callee'
      })
    },

    async pushSignal (channelId, passcode, signal) {
      return post('/poll', {
        channel_id: channelId,
        passcode,
        role: 'caller',
        signal
      })
    },

    async hangup (channelId, passcode, role) {
      return post('/hangup', {
        channel_id: channelId,
        passcode,
        role
      })
    }
  }
}
