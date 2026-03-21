import type { IJsonModel, IJsonTabNode } from '@aptre/flex-layout'
import type { Tab } from '../types'
import type { Application } from '../types'

/**
 * Creates a default FlexLayout JSON model from a flat array of tabs.
 * Used for legacy workspaces that don't have metadata.layoutModel yet.
 */
export function createDefaultLayoutModel(tabs: Tab[], activeTabId: string | null, appLookup?: (applicationId: string) => Application | undefined): IJsonModel {
  const selectedIndex = activeTabId
    ? Math.max(0, tabs.findIndex(t => t.id === activeTabId))
    : 0

  return {
    global: {
      tabEnableRename: false,
      tabEnablePopout: false,
      tabSetEnableMaximize: true,
      tabSetEnableDeleteWhenEmpty: true,
      splitterSize: 4,
      splitterExtra: 4,
      tabEnableRenderOnDemand: false,
    },
    borders: [],
    layout: {
      type: 'row',
      children: [
        {
          type: 'tabset',
          active: true,
          selected: selectedIndex,
          children: tabs.map((tab): IJsonTabNode => tabToFlexNode(tab, appLookup?.(tab.applicationId))),
        },
      ],
    },
  }
}

/**
 * Converts a Tab to a FlexLayout tab node JSON.
 * Optionally accepts an app lookup; if not provided, defaults to allowing close.
 */
export function tabToFlexNode(tab: Tab, app?: Application): IJsonTabNode {
  const canClose = canCloseTabInLayout(app)

  return {
    type: 'tab',
    id: tab.id,
    name: tab.title,
    component: tab.applicationId,
    enableClose: canClose,
  }
}

function canCloseTabInLayout(app?: Application): boolean {
  if (!app?.canClose) return false
  if (app.canHaveMultiple) return true
  if (!app.isDefault) return true
  // For default single-instance apps, we allow close in the layout model
  // but the store's removeTab will handle the actual close validation
  return true
}
