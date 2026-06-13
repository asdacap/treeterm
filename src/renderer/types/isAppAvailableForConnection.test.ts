import { describe, it, expect } from 'vitest'
import { AppAvailability, isAppAvailableForConnection } from './index'

describe('isAppAvailableForConnection', () => {
  it('always-available apps show on local sessions', () => {
    expect(isAppAvailableForConnection({ availability: AppAvailability.Always }, false)).toBe(true)
  })

  it('apps without an availability field show on local sessions', () => {
    expect(isAppAvailableForConnection({}, false)).toBe(true)
  })

  it('remote-only apps are hidden on local sessions', () => {
    expect(isAppAvailableForConnection({ availability: AppAvailability.RemoteOnly }, false)).toBe(false)
  })

  it('remote-only apps are shown on remote sessions', () => {
    expect(isAppAvailableForConnection({ availability: AppAvailability.RemoteOnly }, true)).toBe(true)
  })
})
