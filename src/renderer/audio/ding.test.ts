import { describe, expect, it, vi } from 'vitest'
import { createDingPlayer } from './ding'

function fakeAudio(initialState = 'running'): {
  context: AudioContext
  oscillator: { onended: (() => void) | null; disconnect: ReturnType<typeof vi.fn>; stop: ReturnType<typeof vi.fn> }
  resume: ReturnType<typeof vi.fn>
  close: ReturnType<typeof vi.fn>
} {
  const oscillator = { frequency: { setValueAtTime: vi.fn() }, connect: vi.fn(), disconnect: vi.fn(), start: vi.fn(), stop: vi.fn(), onended: null as (() => void) | null }
  const gain = { gain: { setValueAtTime: vi.fn(), exponentialRampToValueAtTime: vi.fn() }, connect: vi.fn(), disconnect: vi.fn() }
  const context = {
    state: initialState, currentTime: 0, destination: {},
    resume: vi.fn((): Promise<void> => { context.state = 'running'; return Promise.resolve() }),
    close: vi.fn((): Promise<void> => { context.state = 'closed'; return Promise.resolve() }),
    createOscillator: () => oscillator, createGain: () => gain
  }
  return { context: context as unknown as AudioContext, oscillator, resume: context.resume, close: context.close }
}

describe('ding audio adapter', () => {
  it('plays a short tone and releases its audio resources', async () => {
    const audio = fakeAudio()
    const player = createDingPlayer(() => audio.context)
    const playing = player.playDing()
    expect(audio.oscillator.stop).toHaveBeenCalledWith(0.45)
    audio.oscillator.onended?.()
    await playing
    expect(audio.oscillator.disconnect).toHaveBeenCalledOnce()
    expect(audio.close).toHaveBeenCalledOnce()
  })
  it('resumes suspended audio', async () => {
    const audio = fakeAudio('suspended')
    const playing = createDingPlayer(() => audio.context).playDing()
    await Promise.resolve()
    audio.oscillator.onended?.()
    await playing
    expect(audio.resume).toHaveBeenCalledOnce()
  })
  it('reports blocked audio after resume', async () => {
    const audio = fakeAudio('suspended')
    audio.resume.mockReturnValue(Promise.resolve())
    await expect(createDingPlayer(() => audio.context).playDing()).rejects.toThrow('blocked or unavailable')
    expect(audio.close).toHaveBeenCalledOnce()
  })
  it('reports rejected resume and cleans up', async () => {
    const audio = fakeAudio('suspended')
    audio.resume.mockRejectedValue(new Error('not allowed'))
    await expect(createDingPlayer(() => audio.context).playDing()).rejects.toThrow('not allowed')
    expect(audio.close).toHaveBeenCalledOnce()
  })
  it('reports unavailable audio construction', async () => {
    await expect(createDingPlayer(() => { throw new Error('no audio device') }).playDing()).rejects.toThrow('no audio device')
  })
  it('disposes active playback without waiting for ended and rejects further playback', async () => {
    const audio = fakeAudio()
    const player = createDingPlayer(() => audio.context)
    const playing = player.playDing()
    await player.dispose()
    await playing
    expect(audio.close).toHaveBeenCalledOnce()
    await expect(player.playDing()).rejects.toThrow('disposed')
    await player.dispose()
  })
})
