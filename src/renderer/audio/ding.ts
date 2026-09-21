/* eslint-disable custom/no-string-literal-comparison -- Web Audio exposes browser-defined string states, not our enums. */
export interface DingPlayer {
  playDing: () => Promise<void>
  dispose: () => Promise<void>
}

/** Inject at the application root. Callers must surface rejected playback to the user. */
export function createDingPlayer(createContext: () => AudioContext = () => new AudioContext()): DingPlayer {
  let disposed = false
  const contexts = new Map<AudioContext, () => void>()
  return {
    playDing: async (): Promise<void> => {
      if (disposed) throw new Error('Notification audio has been disposed')
      const context = createContext()
      contexts.set(context, () => {})
      try {
        if (context.state !== 'running') await context.resume()
        // dispose() can run while resume() is pending.
        // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- async teardown changes this closure flag
        if (disposed) throw new Error('Notification audio has been disposed')
        if (context.state !== 'running') throw new Error('Notification audio is blocked or unavailable')
        const oscillator = context.createOscillator()
        const gain = context.createGain()
        try {
          oscillator.frequency.setValueAtTime(880, context.currentTime)
          gain.gain.setValueAtTime(0.15, context.currentTime)
          gain.gain.exponentialRampToValueAtTime(0.001, context.currentTime + 0.4)
          oscillator.connect(gain)
          gain.connect(context.destination)
          oscillator.start()
          oscillator.stop(context.currentTime + 0.45)
          await new Promise<void>(resolve => {
            contexts.set(context, resolve)
            oscillator.onended = (): void => { resolve() }
          })
        } finally {
          oscillator.disconnect()
          gain.disconnect()
        }
      } finally {
        contexts.delete(context)
        if (context.state !== 'closed') await context.close()
      }
    },
    dispose: async (): Promise<void> => {
      disposed = true
      await Promise.all(Array.from(contexts, async ([context, finish]): Promise<void> => {
        if (context.state !== 'closed') await context.close()
        finish()
      }))
      contexts.clear()
    }
  }
}
