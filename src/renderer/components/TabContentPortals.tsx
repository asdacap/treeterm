import { useState, useEffect } from 'react'
import { createPortal } from 'react-dom'
import type { StoreApi } from 'zustand'
import { useStore } from 'zustand'
import type { SessionState } from '../store/createSessionStore'
import { useAppStore } from '../store/app'
import { SessionStoreContext } from '../contexts/SessionStoreContext'
import { ErrorBoundary } from './ErrorBoundary'
import TabErrorFallback from './TabErrorFallback'
import { getTabs } from '../types'

interface TabContentPortalsProps {
  sessionStore: StoreApi<SessionState>
  activeWorkspaceId: string | null
}

/**
 * Renders all tab content as sibling divs (keepAlive pattern),
 * then portals visible tabs into their FlexLayout slot elements.
 *
 * Visibility is determined by portal slot presence in the DOM —
 * FlexLayout creates these slots when a tab is selected/visible.
 */
export default function TabContentPortals({ sessionStore, activeWorkspaceId }: TabContentPortalsProps) {
  const workspaces = useStore(sessionStore, s => s.workspaces)
  const workspaceStores = useStore(sessionStore, s => s.workspaceStores)
  const applications = useAppStore((s) => s.applications)

  // Track available portal slots — updated via MutationObserver
  const [portalSlots, setPortalSlots] = useState<Record<string, HTMLElement>>({})

  // Observe DOM for flexlayout-slot-* elements appearing/disappearing
  useEffect(() => {
    const updateSlots = () => {
      const slots: Record<string, HTMLElement> = {}
      document.querySelectorAll<HTMLElement>('[id^="flexlayout-slot-"]').forEach(el => {
        const tabId = el.id.replace('flexlayout-slot-', '')
        slots[tabId] = el
      })
      setPortalSlots(prev => {
        // Only update if slots actually changed
        const prevKeys = Object.keys(prev).sort().join(',')
        const newKeys = Object.keys(slots).sort().join(',')
        if (prevKeys === newKeys) return prev
        return slots
      })
    }

    // Initial scan
    updateSlots()

    // Observe mutations for slot changes
    const observer = new MutationObserver(updateSlots)
    observer.observe(document.body, { childList: true, subtree: true })

    return () => observer.disconnect()
  }, [])

  // Collect tab IDs for the active workspace to detect unassigned portal slots
  const activeWorkspace = activeWorkspaceId ? workspaces[activeWorkspaceId] : null
  const activeTabIds = activeWorkspace ? new Set(getTabs(activeWorkspace).map(t => t.id)) : new Set<string>()

  return (
    <SessionStoreContext.Provider value={sessionStore}>
      {Object.values(workspaces).map(workspace => {
        const wsTabs = getTabs(workspace)
        const isActiveWorkspace = workspace.id === activeWorkspaceId

        return wsTabs.map(tab => {
          const app = applications[tab.applicationId]
          if (!app) return null

          // All tabs stay mounted (hidden when workspace is inactive)

          const portalTarget = isActiveWorkspace ? portalSlots[tab.id] : null
          const isVisible = !!portalTarget

          const handle = workspaceStores[workspace.id]
          if (!handle) return null

          const content = (
            <ErrorBoundary
              key={`error-${workspace.id}-${tab.id}`}
              fallback={(error, reset) => (
                <TabErrorFallback
                  error={error}
                  tabTitle={tab.title}
                  onReset={reset}
                  onClose={() => handle.getState().removeTab(tab.id)}
                />
              )}
            >
              <div
                className={`app-wrapper ${tab.applicationId}-wrapper`}
                style={{ display: isVisible ? app.displayStyle : 'none', height: '100%', width: '100%' }}
              >
                {app.render({
                  tab,
                  workspace: handle,
                  isVisible,
                })}
              </div>
            </ErrorBoundary>
          )

          // If this tab has a portal slot in FlexLayout, portal into it
          if (portalTarget) {
            return createPortal(content, portalTarget, `${workspace.id}-${tab.id}`)
          }

          // Render hidden for tabs in inactive workspaces
          if (!isActiveWorkspace) {
            return (
              <div key={`${workspace.id}-${tab.id}`} style={{ display: 'none' }}>
                {content}
              </div>
            )
          }

          return null
        })
      })}
      {Object.entries(portalSlots).map(([slotId, slotEl]) => {
        if (activeTabIds.has(slotId)) return null
        return createPortal(
          <div className="tab-error-fallback">
            <div className="tab-error-content">
              <h3>No View</h3>
              <p>No view is assigned to this tab.</p>
            </div>
          </div>,
          slotEl,
          `orphan-${slotId}`,
        )
      })}
    </SessionStoreContext.Provider>
  )
}
