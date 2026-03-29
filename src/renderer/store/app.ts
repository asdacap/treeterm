import { create } from 'zustand'
import type { StoreApi } from 'zustand'
import { createSessionStore } from './createSessionStore'
import type { SessionState, SessionEntry } from './createSessionStore'
import { getUnmergedSubWorkspaces } from './createSessionStore'
import { useSettingsStore } from './settings'
import { useKeybindingStore } from './keybinding'
import { initKeyboardHealthMonitor } from '../utils/keyboardHealthMonitor'
import { useNavigationStore } from './navigation'
import { useActivityStateStore } from './activityState'
import { useSessionNamesStore } from './sessionNames'
import { createTerminalApplication, createTerminalVariant } from '../../applications/terminal/renderer'
import { filesystemApplication } from '../../applications/filesystem/renderer'
import { createAiHarnessVariant } from '../../applications/aiHarness/renderer'
import { reviewApplication } from '../../applications/review/renderer'
import { editorApplication } from '../../applications/editor/renderer'
import { commentsApplication } from '../../applications/comments/renderer'
import { chatApplication } from '../../applications/chat/renderer'
import { systemPromptDebuggerApplication } from '../../applications/terminalAnalyzerDebugger/renderer'
import { analyzerHistoryApplication } from '../../applications/analyzerHistory/renderer'
import { workspaceSettingsApplication } from '../../applications/workspaceSettings/renderer'
import { githubApplication } from '../../applications/github/renderer'
import type {
  Workspace, Session, Application,
  Platform, TerminalApi, GitApi, SessionApi, AppApi, DaemonApi,
  FilesystemApi, STTApi, SandboxApi, SettingsApi, RunActionsApi,
  TerminalInstance, AiHarnessInstance,
  ConnectionInfo, SSHConnectionConfig, SSHApi, LlmApi, ClipboardApi, GitHubApi
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
  llm: LlmApi
  clipboard: ClipboardApi
  github: GitHubApi
  selectFolder: () => Promise<string | null>
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
  registerTerminalVariants: (instances: TerminalInstance[]) => void
  registerAiHarnessVariants: (instances: AiHarnessInstance[]) => void

  // Session management
  sessionStores: Record<string, SessionEntry>

  // Actions
  initialize: (deps: AppDeps) => Promise<() => void>
  disconnectSession: (sessionId: string) => void
  addRemoteSession: (session: Session, connection: ConnectionInfo) => Promise<void>
  startRemoteConnect: (config: SSHConnectionConfig) => void
  setSessionError: (connectionId: string, error: string) => void
  removeSession: (id: string) => void

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
  llm: UNINITIALIZED,
  clipboard: UNINITIALIZED,
  github: UNINITIALIZED,
  selectFolder: UNINITIALIZED,
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
    get().registerApplication(createTerminalApplication(deps))
    get().registerApplication(filesystemApplication)
    get().registerApplication(reviewApplication)
    get().registerApplication(editorApplication)
    get().registerApplication(commentsApplication)
    get().registerApplication(chatApplication)
    get().registerApplication(systemPromptDebuggerApplication)
    get().registerApplication(analyzerHistoryApplication)
    get().registerApplication(workspaceSettingsApplication)
    get().registerApplication(githubApplication)
  },

  registerTerminalVariants: (instances: TerminalInstance[]) => {
    const { terminal } = get()
    const deps = { terminal: { kill: terminal.kill.bind(terminal) } }

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
    useKeybindingStore.getState().init()
    initKeyboardHealthMonitor()

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
      const allUnmerged: Workspace[] = []
      for (const entry of Object.values(get().sessionStores)) {
        if (entry.status === 'connected') {
          allUnmerged.push(...getUnmergedSubWorkspaces(entry.store.getState().workspaces))
        }
      }
      if (allUnmerged.length > 0) {
        set({ unmergedWorkspaces: allUnmerged, showCloseConfirm: true })
      } else {
        appApi.confirmClose()
      }
    })

    const unsubReady = appApi.onReady((session) => {
      console.log('[App] Received app:ready with session:', session?.id)
      if (session) {
        const sessionStore = getOrCreateSession(session.id, get, set)
        if (!useSessionNamesStore.getState().getName(session.id)) {
          useSessionNamesStore.getState().setName(session.id, 'LOCAL')
        }
        if (session.workspaces && session.workspaces.length > 0) {
          sessionStore.getState().handleRestore(session)
          const firstWs = session.workspaces[0]
          useNavigationStore.getState().setActiveView({ type: 'workspace', workspaceId: firstWs.id, sessionId: session.id })
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
      const { activeView } = useNavigationStore.getState()
      const sessionId = activeView?.type === 'workspace' ? activeView.sessionId : null
      const sessionEntry = sessionId ? get().sessionStores[sessionId] : Object.values(get().sessionStores)[0]
      if (sessionEntry?.status === 'connected') {
        const { workspaces, addWorkspace, setActiveWorkspace } = sessionEntry.store.getState()
        const existingId = Object.entries(workspaces).find(
          ([, e]) => (e.status === 'loaded' || e.status === 'operation-error') && e.data.path === initialPath
        )?.[0]
        if (existingId) {
          setActiveWorkspace(existingId)
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

  disconnectSession: (sessionId: string) => {
    set((state) => {
      const { [sessionId]: _, ...remainingSessions } = state.sessionStores
      return { sessionStores: remainingSessions }
    })
    // Clear navigation if it pointed to this session
    const { activeView } = useNavigationStore.getState()
    if (activeView && 'sessionId' in activeView && activeView.sessionId === sessionId) {
      const remainingIds = Object.keys(get().sessionStores)
      if (remainingIds.length > 0) {
        const nextEntry = get().sessionStores[remainingIds[0]]
        if (nextEntry.status === 'connected') {
          const workspaces = nextEntry.store.getState().workspaces
          const firstWsId = Object.keys(workspaces)[0]
          if (firstWsId) {
            useNavigationStore.getState().setActiveView({ type: 'workspace', workspaceId: firstWsId, sessionId: remainingIds[0] })
          }
        } else {
          useNavigationStore.getState().setActiveView({ type: 'session', sessionId: remainingIds[0] })
        }
      }
    }
  },

  addRemoteSession: async (session: Session, connection: ConnectionInfo) => {
    console.log(`[renderer:app] addRemoteSession called: session=${session.id}, connection=${connection.id}, status=${connection.status}, workspaces=${session.workspaces?.length ?? 0}`)
    // Remove the old connecting entry (keyed by connectionId) and create connected entry (keyed by session.id)
    const store = getOrCreateSession(session.id, get, set, connection)
    if (!useSessionNamesStore.getState().getName(session.id)) {
      const label = connection.target.type === 'remote'
        ? (connection.target.config.label || `${connection.target.config.user}@${connection.target.config.host}`)
        : session.id
      useSessionNamesStore.getState().setName(session.id, label)
    }
    // Remove old connecting entry if it was keyed differently
    if (connection.id !== session.id) {
      set((state) => {
        const { [connection.id]: _, ...rest } = state.sessionStores
        return { sessionStores: { ...rest, [session.id]: { status: 'connected' as const, store } } }
      })
    }
    console.log(`[renderer:app] Session store created/retrieved for session=${session.id}`)
    if (session.workspaces && session.workspaces.length > 0) {
      console.log(`[renderer:app] Restoring ${session.workspaces.length} workspaces for session=${session.id}`)
      store.getState().handleRestore(session)
      const firstWs = session.workspaces[0]
      useNavigationStore.getState().setActiveView({ type: 'workspace', workspaceId: firstWs.id, sessionId: session.id })
    } else if (connection.target.type === 'remote') {
      const defaultPath = `/home/${connection.target.config.user}`
      console.log(`[renderer:app] No workspaces for session=${session.id}, creating default workspace at ${defaultPath}`)
      const workspaceId = await store.getState().addWorkspace(defaultPath)
      useNavigationStore.getState().setActiveView({ type: 'workspace', workspaceId, sessionId: session.id })
    } else {
      console.log(`[renderer:app] No workspaces to restore for session=${session.id}`)
    }
  },

  startRemoteConnect: (config: SSHConnectionConfig) => {
    set((state) => ({
      sessionStores: {
        ...state.sessionStores,
        [config.id]: { status: 'connecting' as const, connectionId: config.id, config }
      }
    }))
    useNavigationStore.getState().setActiveView({ type: 'session', sessionId: config.id })
  },

  setSessionError: (connectionId: string, error: string) => {
    const entry = get().sessionStores[connectionId]
    if (entry?.status === 'connecting') {
      set((state) => ({
        sessionStores: {
          ...state.sessionStores,
          [connectionId]: { status: 'error' as const, connectionId: entry.connectionId, config: entry.config, error }
        }
      }))
    }
  },

  removeSession: (id: string) => {
    set((state) => {
      const { [id]: _, ...rest } = state.sessionStores
      return { sessionStores: rest }
    })
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

// Helper: get or create a connected session store
function getOrCreateSession(
  sessionId: string,
  get: () => AppState,
  set: (partial: Partial<AppState> | ((state: AppState) => Partial<AppState>)) => void,
  connection?: ConnectionInfo
): StoreApi<SessionState> {
  const { sessionStores, windowUuid, git, filesystem, sessionApi, terminal, llm, github } = get()
  const existing = sessionStores[sessionId]
  if (existing?.status === 'connected') {
    console.log(`[renderer:app] getOrCreateSession: reusing existing session store for session=${sessionId}`)
    return existing.store
  }
  console.log(`[renderer:app] getOrCreateSession: creating new session store for session=${sessionId}, connection=${connection?.id ?? 'local'}`)
  const store = createSessionStore(
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
      github,
      llm: { analyzeTerminal: llm.analyzeTerminal, generateTitle: llm.generateTitle },
      setActivityTabState: (tabId, state) => useActivityStateStore.getState().setTabState(tabId, state),
    }
  )
  set((state) => ({
    sessionStores: { ...state.sessionStores, [sessionId]: { status: 'connected' as const, store } }
  }))
  console.log(`[renderer:app] getOrCreateSession: session store added to sessionStores, total sessions=${Object.keys(get().sessionStores).length}`)
  return store
}
