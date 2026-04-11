import React, { useCallback, useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { Layout, type ITabSetRenderValues, type ITabRenderValues } from '@aptre/flex-layout'
import { Model, Actions, TabNode, TabSetNode, BorderNode, DockLocation, type Action } from '@aptre/flex-layout'
import type { IJsonModel } from '@aptre/flex-layout'
import { useStore } from 'zustand'
import type { WorkspaceStore } from '../store/createWorkspaceStore'
import { useAppStore } from '../store/app'
import { createDefaultLayoutModel, tabToFlexNode } from '../utils/layoutModel'
import { TabActivityIndicator } from './TabActivityIndicator'
import { getTabs } from '../types'

interface FlexLayoutPaneProps {
  workspace: WorkspaceStore
  onNewTab: (applicationId: string) => void
}

function buildModel(ws: WorkspaceStore, getApplication: (id: string) => ReturnType<ReturnType<typeof useAppStore.getState>['applications']['get']>): Model | null {
  const currentWorkspace = ws.getState().workspace

  const currentTabs = getTabs(currentWorkspace)
  const currentActiveTabId = currentWorkspace.activeTabId ?? null
  let json: IJsonModel
  const saved = currentWorkspace.metadata.layoutModel
  if (saved) {
    try {
      json = JSON.parse(saved) as IJsonModel
    } catch {
      json = createDefaultLayoutModel(currentTabs, currentActiveTabId, getApplication)
    }
  } else {
    json = createDefaultLayoutModel(currentTabs, currentActiveTabId, getApplication)
  }

  return Model.fromJson(json)
}

export default function FlexLayoutPane({ workspace: ws, onNewTab }: FlexLayoutPaneProps) {
  const workspace = useStore(ws, s => s.workspace)
  const removeTab = useStore(ws, s => s.removeTab)
  const setActiveTab = useStore(ws, s => s.setActiveTab)
  const updateMetadata = useStore(ws, s => s.updateMetadata)
  const applications = useAppStore((s) => s.applications)
  const getApplication = useCallback((id: string) => applications.get(id), [applications])
  const menuApplications = Array.from(applications.values()).filter((app) => app.showInNewTabMenu)

  const activeTabId = workspace.activeTabId ?? null

  // Initialize model — recreate when workspace changes (setState-during-render pattern)
  const [model, setModel] = useState<Model | null>(() => buildModel(ws, getApplication))
  const [modelWorkspaceId, setModelWorkspaceId] = useState(workspace.id)
  if (workspace.id !== modelWorkspaceId) {
    setModelWorkspaceId(workspace.id)
    setModel(buildModel(ws, getApplication))
  }

  // Suppress model→store sync while we're applying store→model updates
  const suppressModelChangeRef = useRef(false)

  // Sync store tab changes → model (adds/removes)
  const appStates = workspace.appStates
  useEffect(() => {
    if (!model) return

    const currentWorkspace = ws.getState().workspace
    const currentTabs = getTabs(currentWorkspace)
    const currentIds = new Set(currentTabs.map(t => t.id))

    // Derive already-synced tab IDs from the model itself
    const modelTabIds = new Set<string>()
    model.visitNodes(node => { if (node instanceof TabNode) modelTabIds.add(node.getId()) })

    // Detect added tabs
    const added = currentTabs.filter(t => !modelTabIds.has(t.id))
    // Detect removed tabs
    const removed = Array.from(modelTabIds).filter(id => !currentIds.has(id))

    suppressModelChangeRef.current = true

    const findTabset = (): TabSetNode | null => {
      const active = model.getActiveTabset()
      if (active) return active
      // Fallback: find any tabset in the model
      let tabset: TabSetNode | null = null
      model.visitNodes(node => {
        if (!tabset && node instanceof TabSetNode) {
          tabset = node
        }
      })
      return tabset
    }

    for (const tab of added) {
      const tabset = findTabset()
      if (tabset) {
        const json = tabToFlexNode(tab, getApplication(tab.applicationId))
        model.doAction(Actions.addNode(json, tabset.getId(), DockLocation.CENTER, -1, true))
      }
    }

    for (const id of removed) {
      const node = model.getNodeById(id)
      if (node) {
        model.doAction(Actions.deleteTab(id))
      }
    }

    // Reconcile: ensure all store tabs exist in the model
    // Handles tabs that exist in syncedTabIdsRef but were never added to the model
    // (e.g. initial add failed)
    let reconciled = false
    for (const tab of currentTabs) {
      if (!model.getNodeById(tab.id)) {
        const tabset = findTabset()
        if (tabset) {
          const json = tabToFlexNode(tab, getApplication(tab.applicationId))
          model.doAction(Actions.addNode(json, tabset.getId(), DockLocation.CENTER, -1, true))
          reconciled = true
        }
      }
    }

    suppressModelChangeRef.current = false

    // Persist final model state after bulk mutations (onModelChange was suppressed during mutations)
    if (added.length > 0 || removed.length > 0 || reconciled) {
      updateMetadata('layoutModel', JSON.stringify(model.toJson()))
    }
  }, [model, appStates, getApplication, ws, updateMetadata])

  // Sync store active tab → model selected tab
  useEffect(() => {
    if (!model || !activeTabId) return

    const node = model.getNodeById(activeTabId)
    if (node instanceof TabNode && !node.isSelected()) {
      suppressModelChangeRef.current = true
      model.doAction(Actions.selectTab(activeTabId))
      suppressModelChangeRef.current = false
    }
  }, [model, activeTabId])

  // Factory: renders a portal target div for each tab
  const factory = useCallback((node: TabNode) => {
    const tabId = node.getId()
    return <div id={`flexlayout-slot-${tabId}`} style={{ height: '100%', width: '100%' }} />
  }, [])

  // Handle actions from FlexLayout (intercept delete to route through store)
  const handleAction = useCallback((action: Action): Action | undefined => {
    if (action.type === Actions.DELETE_TAB) {
      const tabId = action.data.node
      if (!workspace.appStates[tabId]) {
        return action // Orphan tab — let FlexLayout remove it directly
      }
      void removeTab(tabId)
      return undefined // Prevent FlexLayout from handling it — store will sync
    }
    if (action.type === Actions.SELECT_TAB) {
      setActiveTab(action.data.tabNode)
    }
    return action
  }, [workspace, removeTab, setActiveTab])

  // Serialize model changes to metadata
  const handleModelChange = useCallback((m: Model) => {
    if (suppressModelChangeRef.current) return
    const json = JSON.stringify(m.toJson())
    updateMetadata('layoutModel', json)
  }, [updateMetadata])

  // Customize tab rendering with icons and activity indicators
  const handleRenderTab = useCallback((node: TabNode, renderValues: ITabRenderValues) => {
    const tabId = node.getId()
    const wsState = ws.getState().workspace
    const tab = getTabs(wsState).find(t => t.id === tabId)
    if (tab) {
      const app = getApplication(tab.applicationId)
      if (app) {
        renderValues.leading = <span className="tab-icon">{app.icon}</span>
      }
      renderValues.buttons.push(
        <TabActivityIndicator key="activity" tabId={tabId} />
      )
    }
  }, [getApplication, ws])

  // Menu state: anchor position for the portal-rendered dropdown
  const [menuAnchor, setMenuAnchor] = useState<{ top: number; left: number; right: number } | null>(null)

  // Add "+" button to each tabset header; menu renders via portal
  const handleRenderTabSet = useCallback((node: TabSetNode | BorderNode, renderValues: ITabSetRenderValues) => {
    if (node instanceof TabSetNode) {
      renderValues.stickyButtons.push(
        <button
          key="new-tab"
          className="flexlayout-new-tab-btn"
          onClick={(e) => {
            if (menuAnchor) {
              setMenuAnchor(null)
            } else {
              const rect = (e.target as HTMLElement).getBoundingClientRect()
              setMenuAnchor({ top: rect.bottom + 4, left: rect.left, right: rect.right })
            }
          }}
          title="New tab"
        >
          +
        </button>
      )
    }
  }, [menuAnchor])

  if (!model) return null

  return (
    <>
      <Layout
        model={model}
        factory={factory}
        onAction={handleAction}
        onModelChange={handleModelChange}
        onRenderTab={handleRenderTab}
        onRenderTabSet={handleRenderTabSet}
      />
      {menuAnchor && (
        <NewTabDropdownMenu
          anchor={menuAnchor}
          applications={menuApplications}
          onNewTab={(appId) => { onNewTab(appId); setMenuAnchor(null) }}
          onClose={() => { setMenuAnchor(null); }}
        />
      )}
    </>
  )
}

/** Self-contained dropdown menu that handles its own click-outside detection */
function NewTabDropdownMenu({ anchor, applications, onNewTab, onClose }: {
  anchor: { top: number; left: number; right: number }
  applications: { id: string; icon: string; name: string }[]
  onNewTab: (appId: string) => void
  onClose: () => void
}) {
  const menuRef = React.useRef<HTMLDivElement>(null)

  React.useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        onClose()
      }
    }
    document.addEventListener('mousedown', handleClickOutside)
    return () => { document.removeEventListener('mousedown', handleClickOutside); }
  }, [onClose])

  return createPortal(
    <div
      className="app-menu"
      ref={menuRef}
      style={{
        position: 'fixed',
        top: anchor.top,
        right: 'auto',
        ...(anchor.right >= 140
          ? { left: anchor.right, transform: 'translateX(-100%)' }
          : { left: anchor.left }),
      }}
    >
      {applications.map((app) => (
        <div
          key={app.id}
          className="app-menu-item"
          onClick={() => { onNewTab(app.id); }}
        >
          <span className="app-menu-icon">{app.icon}</span>
          <span className="app-menu-name">{app.name}</span>
        </div>
      ))}
    </div>,
    document.body
  )
}
