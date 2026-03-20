import { describe, it, expect, beforeEach } from 'vitest'
import { useNavigationStore } from './navigation'

describe('navigation store', () => {
  beforeEach(() => {
    useNavigationStore.setState({ activeView: null })
  })

  it('starts with null activeView', () => {
    expect(useNavigationStore.getState().activeView).toBeNull()
  })

  it('sets workspace view', () => {
    useNavigationStore.getState().setActiveView({ type: 'workspace', workspaceId: 'ws-1' })
    expect(useNavigationStore.getState().activeView).toEqual({
      type: 'workspace',
      workspaceId: 'ws-1',
    })
  })

  it('sets ssh view', () => {
    useNavigationStore.getState().setActiveView({ type: 'ssh', connectionId: 'conn-1' })
    expect(useNavigationStore.getState().activeView).toEqual({
      type: 'ssh',
      connectionId: 'conn-1',
    })
  })

  it('switches between views', () => {
    const { setActiveView } = useNavigationStore.getState()
    setActiveView({ type: 'workspace', workspaceId: 'ws-1' })
    setActiveView({ type: 'ssh', connectionId: 'conn-1' })
    expect(useNavigationStore.getState().activeView).toEqual({
      type: 'ssh',
      connectionId: 'conn-1',
    })
  })
})
