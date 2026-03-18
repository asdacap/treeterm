import { describe, it, expect } from 'vitest'
import { createDefaultLayoutModel, tabToFlexNode } from './layoutModel'
import type { Tab } from '../../shared/types'
import type { Application } from '../types'

const testApp: Application = {
  id: 'test-terminal',
  name: 'Terminal',
  icon: '>_',
  createInitialState: () => ({}),
  render: () => null,
  canClose: true,
  canHaveMultiple: true,
  showInNewTabMenu: true,
  keepAlive: true,
  displayStyle: 'flex',
  isDefault: true,
}

const makeTabs = (count: number): Tab[] =>
  Array.from({ length: count }, (_, i) => ({
    id: `tab-${i}`,
    applicationId: 'test-terminal',
    title: `Terminal ${i + 1}`,
    state: {},
  }))

describe('createDefaultLayoutModel', () => {
  it('creates a valid model with a single tabset', () => {
    const tabs = makeTabs(3)
    const model = createDefaultLayoutModel(tabs, 'tab-0')

    expect(model.layout.type).toBe('row')
    expect(model.layout.children).toHaveLength(1)

    const tabset = model.layout.children[0]
    expect(tabset.type).toBe('tabset')
    if (tabset.type === 'tabset') {
      expect(tabset.children).toHaveLength(3)
      expect(tabset.selected).toBe(0)
      expect(tabset.active).toBe(true)
    }
  })

  it('selects the correct tab by activeTabId', () => {
    const tabs = makeTabs(3)
    const model = createDefaultLayoutModel(tabs, 'tab-2')

    const tabset = model.layout.children[0]
    if (tabset.type === 'tabset') {
      expect(tabset.selected).toBe(2)
    }
  })

  it('defaults to index 0 when activeTabId is null', () => {
    const tabs = makeTabs(2)
    const model = createDefaultLayoutModel(tabs, null)

    const tabset = model.layout.children[0]
    if (tabset.type === 'tabset') {
      expect(tabset.selected).toBe(0)
    }
  })

  it('defaults to index 0 when activeTabId is not found', () => {
    const tabs = makeTabs(2)
    const model = createDefaultLayoutModel(tabs, 'nonexistent')

    const tabset = model.layout.children[0]
    if (tabset.type === 'tabset') {
      expect(tabset.selected).toBe(0)
    }
  })

  it('creates model with empty tabs', () => {
    const model = createDefaultLayoutModel([], null)

    const tabset = model.layout.children[0]
    if (tabset.type === 'tabset') {
      expect(tabset.children).toHaveLength(0)
    }
  })

  it('disables popout and rename globally', () => {
    const model = createDefaultLayoutModel(makeTabs(1), null)
    expect(model.global?.tabEnableRename).toBe(false)
    expect(model.global?.tabEnablePopout).toBe(false)
  })

  it('disables render-on-demand to support portal pattern', () => {
    const model = createDefaultLayoutModel(makeTabs(1), null)
    expect(model.global?.tabEnableRenderOnDemand).toBe(false)
  })
})

describe('tabToFlexNode', () => {
  it('converts a tab to a flex node with correct properties', () => {
    const tab: Tab = {
      id: 'tab-1',
      applicationId: 'test-terminal',
      title: 'My Terminal',
      state: {},
    }

    const node = tabToFlexNode(tab, testApp)

    expect(node.type).toBe('tab')
    expect(node.id).toBe('tab-1')
    expect(node.name).toBe('My Terminal')
    expect(node.component).toBe('test-terminal')
    expect(node.enableClose).toBe(true)
  })

  it('handles missing application gracefully', () => {
    const tab: Tab = {
      id: 'tab-2',
      applicationId: 'unknown-app',
      title: 'Unknown',
      state: {},
    }

    const node = tabToFlexNode(tab)

    expect(node.type).toBe('tab')
    expect(node.id).toBe('tab-2')
    expect(node.enableClose).toBe(false)
  })
})
