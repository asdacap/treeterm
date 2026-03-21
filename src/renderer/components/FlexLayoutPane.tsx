import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { Layout, type ITabSetRenderValues, type ITabRenderValues } from '@aptre/flex-layout'
import { Model, Actions, TabNode, TabSetNode, BorderNode, DockLocation, type Action } from '@aptre/flex-layout'
import type { IJsonModel } from '@aptre/flex-layout'
import { useStore } from 'zustand'
import type { WorkspaceHandle } from '../store/createWorkspaceHandleStore'
import { useAppStore } from '../store/app'
import { createDefaultLayoutModel, tabToFlexNode } from '../utils/layoutModel'
import { TabActivityIndicator } from './TabActivityIndicator'
import { getTabs } from '../types'

interface FlexLayoutPaneProps {
  workspace: WorkspaceHandle
  onNewTab: (applicationId: string) => void
}

export default function FlexLayoutPane({ workspace: ws, onNewTab }: FlexLayoutPaneProps) {
  const { workspace, removeTab, setActiveTab, updateMetadata } = useStore(ws)
  const applications = useAppStore((s) => s.applications)
  const getApplication = useCallback((id: string) => applications[id], [applications])
  const menuApplications = useMemo(() => Object.values(applications).filter((app) => app.showInNewTabMenu), [applications])

  // Menu state: anchor position for the portal-rendered dropdown
  const [menuAnchor, setMenuAnchor] = useState<{ top: number; left: number; right: number } | null>(null)
  const menuRef = useRef<HTMLDivElement>(null)

  // Close menu on click outside
  useEffect(() => {
    if (!menuAnchor) return
    const handleClickOutside = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setMenuAnchor(null)
      }
    }
    document.addEventListener('mousedown', handleClickOutside)
    return () => document.removeEventListener('mousedown', handleClickOutside)
  }, [menuAnchor])

  const tabs = workspace ? getTabs(workspace) : []
  const activeTabId = workspace?.activeTabId ?? null

  const [model, setModel] = useState<Model | null>(null)
  const layoutRef = useRef<Layout>(null)
  // Track tab IDs we've synced into the model to detect adds/removes
  const syncedTabIdsRef = useRef<Set<string>>(new Set())
  // Suppress model→store sync while we're applying store→model updates
  const suppressModelChangeRef = useRef(false)

  // Initialize model from metadata or create default
  useEffect(() => {
    if (!workspace) return

    let json: IJsonModel
    const saved = workspace.metadata?.layoutModel
    if (saved) {
      try {
        json = JSON.parse(saved)
      } catch {
        json = createDefaultLayoutModel(tabs, activeTabId, getApplication)
      }
    } else {
      json = createDefaultLayoutModel(tabs, activeTabId, getApplication)
    }

    const m = Model.fromJson(json)
    setModel(m)
    syncedTabIdsRef.current = new Set(tabs.map(t => t.id))
    // Only run on mount / workspace ID change
  }, [workspace.id])

  // Sync store tab changes → model (adds/removes)
  useEffect(() => {
    if (!model) return

    const currentIds = new Set(tabs.map(t => t.id))
    const synced = syncedTabIdsRef.current

    // Detect added tabs
    const added = tabs.filter(t => !synced.has(t.id))
    // Detect removed tabs
    const removed = Array.from(synced).filter(id => !currentIds.has(id))

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
    // (e.g. canHaveMultiple:false reuse, or initial add failed)
    let reconciled = false
    for (const tab of tabs) {
      if (!model.getNodeById(tab.id)) {
        const tabset = findTabset()
        if (tabset) {
          const json = tabToFlexNode(tab, getApplication(tab.applicationId))
          model.doAction(Actions.addNode(json, tabset.getId(), DockLocation.CENTER, -1, true))
          reconciled = true
        }
      }
    }

    syncedTabIdsRef.current = currentIds
    suppressModelChangeRef.current = false

    // Force re-render after model mutations
    if (added.length > 0 || removed.length > 0 || reconciled) {
      setModel(model)
    }
  }, [model, tabs])

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
      removeTab(action.data.node)
      return undefined // Prevent FlexLayout from handling it — store will sync
    }
    if (action.type === Actions.SELECT_TAB) {
      setActiveTab(action.data.tabNode)
    }
    return action
  }, [removeTab, setActiveTab])

  // Serialize model changes to metadata
  const handleModelChange = useCallback((m: Model, _action: Action) => {
    if (suppressModelChangeRef.current) return
    const json = JSON.stringify(m.toJson())
    updateMetadata('layoutModel', json)
  }, [updateMetadata])

  // Customize tab rendering with icons and activity indicators
  const handleRenderTab = useCallback((node: TabNode, renderValues: ITabRenderValues) => {
    const tabId = node.getId()
    const tab = tabs.find(t => t.id === tabId)
    if (tab) {
      const app = getApplication(tab.applicationId)
      if (app) {
        renderValues.leading = <span className="tab-icon">{app.icon}</span>
      }
      renderValues.buttons.push(
        <TabActivityIndicator key="activity" tabId={tabId} />
      )
    }
  }, [tabs, getApplication])

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
        ref={layoutRef}
        model={model}
        factory={factory}
        onAction={handleAction}
        onModelChange={handleModelChange}
        onRenderTab={handleRenderTab}
        onRenderTabSet={handleRenderTabSet}
      />
      {menuAnchor && createPortal(
        <div
          className="app-menu"
          ref={menuRef}
          style={{
            position: 'fixed',
            top: menuAnchor.top,
            right: 'auto', // override CSS right: 0 so width is content-based
            // Right-align to button if there's room (140px min-width), otherwise left-align
            ...(menuAnchor.right >= 140
              ? { left: menuAnchor.right, transform: 'translateX(-100%)' }
              : { left: menuAnchor.left }),
          }}
        >
          {menuApplications.map((app) => (
            <div
              key={app.id}
              className="app-menu-item"
              onClick={() => {
                onNewTab(app.id)
                setMenuAnchor(null)
              }}
            >
              <span className="app-menu-icon">{app.icon}</span>
              <span className="app-menu-name">{app.name}</span>
            </div>
          ))}
        </div>,
        document.body
      )}
    </>
  )
}
