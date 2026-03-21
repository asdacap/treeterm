import { create } from 'zustand'
import type { StoreApi } from 'zustand'
import { createSessionStore } from './createSessionStore'
import type { SessionState } from './createSessionStore'
import { getUnmergedSubWorkspaces } from './createSessionStore'
import { useSettingsStore } from './settings'
import { createTerminalApplication, createTerminalVariant } from '../../applications/terminal/renderer'
import { filesystemApplication } from '../../applications/filesystem/renderer'
import { createAiHarnessVariant } from '../../applications/aiHarness/renderer'
import { reviewApplication } from '../../applications/review/renderer'
import { editorApplication } from '../../applications/editor/renderer'
import { commentsApplication } from '../../applications/comments/renderer'
import type {
  Workspace, Session, Application,
  Platform, TerminalApi, GitApi, SessionApi, AppApi, DaemonApi,
  FilesystemApi, STTApi, SandboxApi, SettingsApi, RunActionsApi,
  TerminalInstance, AiHarnessInstance, Settings,
  ConnectionInfo, SSHApi
} from '../types'

export interface AppDeps {
  platform: Platform
  terminal: TerminalApi
  git: GitApi
  sessionApi: SessionApi
  settingsApi: SettingsApi
  appApi: AppApi
  daemon: DaemonApi
  filesystem: FilesystemApi
  stt: STTApi
  runActions: RunActionsApi
  sandbox: SandboxApi
  ssh: SSHApi
  selectFolder: () => Promise<string | null>
  getRecentDirectories: () => Promise<string[]>
  getWindowUuid: () => Promise<string>
  getInitialWorkspace: () => Promise<string | null>
}

interface AppState extends AppDeps {
  // Lifecycle
  windowUuid: string | null
  daemonDisconnected: boolean
  isSettingsOpen: boolean
  isActiveProcessesOpen: boolean
  showCloseConfirm: boolean
  unmergedWorkspaces: Workspace[]
  showWorkspacePicker: boolean
  daemonSessions: Session[]
  showConnectionPicker: boolean

  // Application registry
  applications: Record<string, Application>
  registerApplication: (app: Application) => void
  unregisterApplication: (id: string) => void
  getApplication: (id: string) => Application | undefined
  getAllApplications: () => Application[]
  getMenuApplications: () => Application[]
  getDefaultApplications: () => Application[]
  getDefaultApplication: (appId?: string) => Application | null
  initializeApplications: () => void
  registerTerminalVariants: (instances: TerminalInstance[], terminalSettings: Settings['terminal'] | undefined) => void
  registerAiHarnessVariants: (instances: AiHarnessInstance[]) => void

  // Session management
  activeSessionId: string | null
  sessionStores: Record<string, StoreApi<SessionState>>

  // Actions
  initialize: (deps: AppDeps) => Promise<() => void>
  switchSession: (sessionId: string) => void
  addRemoteSession: (session: Session, connection: ConnectionInfo) => void
  getActiveSessionStore: () => StoreApi<SessionState> | null

  // Internal
  createNewSession: () => Promise<void>
}

// Placeholder used before initialize() injects real deps.
// Safe because initialize() is called before any component renders.
const UNINITIALIZED = null as never

export const useAppStore = create<AppState>()((set, get) => ({
  // Injected APIs — overwritten by initialize(deps) before first use
  platform: UNINITIALIZED,
  terminal: UNINITIALIZED,
  git: UNINITIALIZED,
  sessionApi: UNINITIALIZED,
  settingsApi: UNINITIALIZED,
  appApi: UNINITIALIZED,
  daemon: UNINITIALIZED,
  filesystem: UNINITIALIZED,
  stt: UNINITIALIZED,
  runActions: UNINITIALIZED,
  sandbox: UNINITIALIZED,
  ssh: UNINITIALIZED,
  selectFolder: UNINITIALIZED,
  getRecentDirectories: UNINITIALIZED,
  getWindowUuid: UNINITIALIZED,
  getInitialWorkspace: UNINITIALIZED,

  windowUuid: null,
  daemonDisconnected: false,
  isSettingsOpen: false,
  isActiveProcessesOpen: false,
  showCloseConfirm: false,
  unmergedWorkspaces: [],
  showWorkspacePicker: false,
  daemonSessions: [],
  showConnectionPicker: false,
  activeSessionId: null,
  sessionStores: {},

  // Application registry
  applications: {},

  registerApplication: (app: Application) => {
    set((state) => ({
      applications: { ...state.applications, [app.id]: app }
    }))
  },

  unregisterApplication: (id: string) => {
    set((state) => {
      const { [id]: _, ...rest } = state.applications
      return { applications: rest }
    })
  },

  getApplication: (id: string) => {
    return get().applications[id]
  },

  getAllApplications: () => {
    return Object.values(get().applications)
  },

  getMenuApplications: () => {
    return Object.values(get().applications).filter((app) => app.showInNewTabMenu)
  },

  getDefaultApplications: () => {
    return Object.values(get().applications).filter((app) => app.isDefault)
  },

  getDefaultApplication: (appId?: string) => {
    const apps = get().applications
    if (appId) {
      const app = apps[appId]
      if (app) return app
    }
    const allApps = Object.values(apps)
    return allApps.length > 0 ? allApps[0] : null
  },

  initializeApplications: () => {
    const { terminal } = get()
    const deps = { terminal: { kill: terminal.kill.bind(terminal) } }
    get().registerApplication(createTerminalApplication(true, deps))
    get().registerApplication(filesystemApplication)
    get().registerApplication(reviewApplication)
    get().registerApplication(editorApplication)
    get().registerApplication(commentsApplication)
  },

  registerTerminalVariants: (instances: TerminalInstance[], terminalSettings: Settings['terminal'] | undefined) => {
    const { terminal } = get()
    const deps = { terminal: { kill: terminal.kill.bind(terminal) } }

    // Re-register base terminal with updated startByDefault setting
    if (terminalSettings !== undefined) {
      get().registerApplication(createTerminalApplication(terminalSettings.startByDefault, deps))
    }

    // Unregister existing dynamic terminals
    const allApps = Object.values(get().applications)
    for (const app of allApps) {
      if (app.id.startsWith('terminal-')) {
        get().unregisterApplication(app.id)
      }
    }

    // Register new variants
    for (const instance of instances) {
      get().registerApplication(createTerminalVariant(instance, deps))
    }
  },

  registerAiHarnessVariants: (instances: AiHarnessInstance[]) => {
    const { terminal } = get()
    const deps = { terminal: { kill: terminal.kill.bind(terminal) } }

    // Unregister existing dynamic AI Harness apps
    const allApps = Object.values(get().applications)
    for (const app of allApps) {
      if (app.id.startsWith('aiharness-')) {
        get().unregisterApplication(app.id)
      }
    }

    // Register new variants
    for (const instance of instances) {
      get().registerApplication(createAiHarnessVariant(instance, deps))
    }
  },

  initialize: async (deps: AppDeps) => {
    set(deps)
    const { terminal, sessionApi, settingsApi, appApi, daemon, getWindowUuid, getInitialWorkspace } = deps

    get().initializeApplications()
    useSettingsStore.getState().init(settingsApi, terminal.kill.bind(terminal))

    // Fetch this window's UUID
    try {
      const uuid = await getWindowUuid()
      if (uuid) {
        set({ windowUuid: uuid })
        console.log('[App] Window UUID:', uuid)
      }
    } catch (error) {
      console.error('[App] Failed to fetch window UUID:', error)
    }

    const unsubSettings = settingsApi.onOpen(() => {
      set({ isSettingsOpen: true })
    })

    const unsubClose = appApi.onCloseConfirm(() => {
      const sessionStore = get().getActiveSessionStore()
      if (!sessionStore) {
        appApi.confirmClose()
        return
      }
      const unmerged = getUnmergedSubWorkspaces(sessionStore.getState().workspaces)
      if (unmerged.length > 0) {
        set({ unmergedWorkspaces: unmerged, showCloseConfirm: true })
      } else {
        appApi.confirmClose()
      }
    })

    const unsubReady = appApi.onReady((session) => {
      console.log('[App] Received app:ready with session:', session?.id)
      if (session) {
        const sessionStore = getOrCreateSession(session.id, get, set)
        set({ activeSessionId: session.id })
        if (session.workspaces && session.workspaces.length > 0) {
          sessionStore.getState().handleRestore(session)
        }
      }
    })

    const unsubSync = sessionApi.onSync((session) => {
      console.log('[App] Received session:sync with', session.workspaces.length, 'workspaces')
      const sessionStore = getOrCreateSession(session.id, get, set)
      sessionStore.getState().handleExternalUpdate(session)
    })

    const unsubDisconnect = daemon.onDisconnected(() => {
      console.error('[App] Daemon disconnected')
      set({ daemonDisconnected: true })
    })

    const unsubActiveProcesses = terminal.onActiveProcessesOpen(() => {
      set({ isActiveProcessesOpen: true })
    })

    const unsubShowSessions = sessionApi.onShowSessions(async () => {
      try {
        const result = await sessionApi.list()
        if (result.success && result.sessions) {
          set({ daemonSessions: result.sessions, showWorkspacePicker: true })
        }
      } catch (error) {
        console.error('Failed to list daemon sessions:', error)
        alert(`Failed to list sessions: ${error instanceof Error ? error.message : 'Unknown error'}`)
      }
    })

    // Handle initial workspace from CLI
    const initialPath = await getInitialWorkspace()
    if (initialPath) {
      const sessionStore = get().getActiveSessionStore()
      if (sessionStore) {
        const { workspaces, addWorkspace, setActiveWorkspace } = sessionStore.getState()
        const existingWorkspace = Object.values(workspaces).find(ws => ws.path === initialPath)
        if (existingWorkspace) {
          setActiveWorkspace(existingWorkspace.id)
        } else {
          await addWorkspace(initialPath)
        }
      }
    }

    return () => {
      unsubSettings()
      unsubClose()
      unsubReady()
      unsubSync()
      unsubDisconnect()
      unsubActiveProcesses()
      unsubShowSessions()
    }
  },

  switchSession: (sessionId: string) => {
    set({ activeSessionId: sessionId })
  },

  addRemoteSession: (session: Session, connection: ConnectionInfo) => {
    console.log(`[renderer:app] addRemoteSession called: session=${session.id}, connection=${connection.id}, status=${connection.status}, workspaces=${session.workspaces?.length ?? 0}`)
    const store = getOrCreateSession(session.id, get, set, connection)
    console.log(`[renderer:app] Session store created/retrieved for session=${session.id}`)
    set({ activeSessionId: session.id })
    console.log(`[renderer:app] activeSessionId set to ${session.id}`)
    if (session.workspaces && session.workspaces.length > 0) {
      console.log(`[renderer:app] Restoring ${session.workspaces.length} workspaces for session=${session.id}`)
      store.getState().handleRestore(session)
    } else {
      console.log(`[renderer:app] No workspaces to restore for session=${session.id}`)
    }
  },

  getActiveSessionStore: () => {
    const { activeSessionId, sessionStores } = get()
    if (!activeSessionId) return null
    const store = sessionStores[activeSessionId] || null
    if (!store) {
      console.warn(`[renderer:app] getActiveSessionStore: no store found for activeSessionId=${activeSessionId}, available sessions=[${Object.keys(sessionStores).join(', ')}]`)
    }
    return store
  },


  createNewSession: async () => {
    const { sessionApi } = get()
    try {
      const result = await sessionApi.create([])
      if (!result.success || !result.session) {
        throw new Error(result.error || 'Failed to create session')
      }
      await sessionApi.openInNewWindow(result.session.id)
      set({ showWorkspacePicker: false })
    } catch (error) {
      alert(`Failed to create session: ${error instanceof Error ? error.message : 'Unknown error'}`)
    }
  },
}))

// Helper: get or create a session store
function getOrCreateSession(
  sessionId: string,
  get: () => AppState,
  set: (partial: Partial<AppState> | ((state: AppState) => Partial<AppState>)) => void,
  connection?: ConnectionInfo
): StoreApi<SessionState> {
  const { sessionStores, windowUuid, git, filesystem, sessionApi, terminal } = get()
  let store = sessionStores[sessionId]
  if (!store) {
    console.log(`[renderer:app] getOrCreateSession: creating new session store for session=${sessionId}, connection=${connection?.id ?? 'local'}`)
    store = createSessionStore(
      { sessionId, windowUuid, connection },
      {
        git,
        filesystem,
        sessionApi,
        terminal,
        getSettings: () => useSettingsStore.getState().settings,
        appRegistry: {
          get: (id: string) => get().applications[id],
          getDefaultApp: (appId?: string) => get().getDefaultApplication(appId),
        },
      }
    )
    set((state) => ({
      sessionStores: { ...state.sessionStores, [sessionId]: store! }
    }))
    console.log(`[renderer:app] getOrCreateSession: session store added to sessionStores, total sessions=${Object.keys(get().sessionStores).length}`)
  } else {
    console.log(`[renderer:app] getOrCreateSession: reusing existing session store for session=${sessionId}`)
  }
  return store
}
