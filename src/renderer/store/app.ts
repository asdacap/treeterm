import { create } from 'zustand'
import type { StoreApi } from 'zustand'
import { createSessionStore, WorkspaceEntryStatus } from './createSessionStore'
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
import { createCustomRunnerVariant } from '../../applications/customRunner/renderer'
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
  Platform, TerminalApi, RawGitApi, SessionApi, AppApi, DaemonApi,
  RawFilesystemApi, ExecApi, SandboxApi, SettingsApi, RawRunActionsApi,
  TerminalInstance, AiHarnessInstance, CustomRunnerInstance,
  ConnectionInfo, SSHConnectionConfig, SSHApi, LlmApi, ClipboardApi, RawGitHubApi
} from '../types'
import { createBoundGit, createBoundGitHub, createBoundFilesystem, createBoundRunActions } from '../types'
import { ConnectionStatus } from '../../shared/types'

export interface AppDeps {
  platform: Platform
  terminal: TerminalApi
  git: RawGitApi
  sessionApi: SessionApi
  settingsApi: SettingsApi
  appApi: AppApi
  daemon: DaemonApi
  filesystem: RawFilesystemApi
  exec: ExecApi
  runActions: RawRunActionsApi
  sandbox: SandboxApi
  ssh: SSHApi
  llm: LlmApi
  clipboard: ClipboardApi
  github: RawGitHubApi
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
  showConnectionPicker: boolean

  // Application registry
  applications: Map<string, Application>
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
  registerCustomRunnerVariants: (instances: CustomRunnerInstance[]) => void

  // Session management
  sessionStores: Map<string, SessionEntry>

  // Actions
  initialize: (deps: AppDeps) => Promise<() => void>
  disconnectSession: (sessionId: string) => void
  addRemoteSession: (session: Session, connection: ConnectionInfo) => Promise<void>
  startRemoteConnect: (config: SSHConnectionConfig) => void
  setSessionError: (connectionId: string, error: string) => void
  removeSession: (id: string) => void
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
  exec: UNINITIALIZED,
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
  showConnectionPicker: false,
  sessionStores: new Map<string, SessionEntry>(),

  // Application registry
  applications: new Map<string, Application>(),

  registerApplication: (app: Application) => {
    set((state) => ({
      applications: new Map(state.applications).set(app.id, app)
    }))
  },

  unregisterApplication: (id: string) => {
    set((state) => {
      const rest = new Map(state.applications)
      rest.delete(id)
      return { applications: rest }
    })
  },

  getApplication: (id: string) => {
    return get().applications.get(id)
  },

  getAllApplications: () => {
    return Array.from(get().applications.values())
  },

  getMenuApplications: () => {
    return Array.from(get().applications.values()).filter((app) => app.showInNewTabMenu)
  },

  getDefaultApplications: () => {
    return Array.from(get().applications.values()).filter((app) => app.isDefault)
  },

  getDefaultApplication: (appId?: string) => {
    const apps = get().applications
    if (appId && apps.has(appId)) {
      return apps.get(appId) ?? null
    }
    const allApps = Array.from(apps.values())
    return allApps[0] ?? null
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
    const allApps = Array.from(get().applications.values())
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
    const allApps = Array.from(get().applications.values())
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

  registerCustomRunnerVariants: (instances: CustomRunnerInstance[]) => {
    const { terminal } = get()
    const deps = { terminal: { kill: terminal.kill.bind(terminal) } }

    // Unregister existing dynamic custom runner apps
    const allApps = Array.from(get().applications.values())
    for (const app of allApps) {
      if (app.id.startsWith('customrunner-')) {
        get().unregisterApplication(app.id)
      }
    }

    // Register new variants
    for (const instance of instances) {
      get().registerApplication(createCustomRunnerVariant(instance, deps))
    }
  },

  initialize: async (deps: AppDeps) => {
    set(deps)
    const { terminal, sessionApi, settingsApi, appApi, daemon, ssh, getWindowUuid, getInitialWorkspace } = deps

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
      for (const entry of Array.from(get().sessionStores.values())) {
        allUnmerged.push(...getUnmergedSubWorkspaces(entry.store.getState().workspaces))
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
        const localConnection: ConnectionInfo = { id: 'local', target: { type: 'local' }, status: ConnectionStatus.Connected }
        const sessionStore = getOrCreateSession(session.id, get, set, localConnection)
        if (!useSessionNamesStore.getState().getName(session.id)) {
          useSessionNamesStore.getState().setName(session.id, 'LOCAL')
        }
        if (session.workspaces.length > 0) {
          void sessionStore.getState().handleRestore(session)
          // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- length > 0 checked above
          const firstWs = session.workspaces[0]!
          useNavigationStore.getState().setActiveView({ type: 'workspace', workspaceId: firstWs.id, sessionId: session.id })
        }
      }
    })

    const unsubSync = sessionApi.onSync((connectionId, session) => {
      console.log(`[App] Received session:sync from connection ${connectionId} with ${String(session.workspaces.length)} workspaces for session ${session.id}`)
      const entry = get().sessionStores.get(session.id)
      if (!entry) return
      const storeConnId = entry.store.getState().connection?.id ?? 'local'
      if (storeConnId !== connectionId) {
        console.warn('[App] Ignoring session:sync from wrong connection', connectionId, 'expected', storeConnId)
        return
      }
      void entry.store.getState().handleExternalUpdate(session)
    })

    const unsubSshAutoConnected = appApi.onSshAutoConnected((session, connection) => {
      console.log('[App] SSH auto-connected with session:', session.id, 'connection:', connection.id)
      void get().addRemoteSession(session, connection)
    })

    const unsubReconnected = appApi.onConnectionReconnected((session, connection) => {
      console.log('[App] Connection reconnected:', connection.id, 'session:', session.id)
      // Dispose old session and recreate fresh
      disposeSessionForConnection(connection.id, get)
      const newStore = getOrCreateSession(session.id, get, set, connection)
      if (session.workspaces.length > 0) {
        void newStore.getState().handleRestore(session)
        // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- length > 0 checked above
        const firstWs = session.workspaces[0]!
        useNavigationStore.getState().setActiveView({ type: 'workspace', workspaceId: firstWs.id, sessionId: session.id })
      }
    })

    const unsubDisconnect = daemon.onDisconnected(() => {
      console.error('[App] Daemon disconnected')
      set({ daemonDisconnected: true })
    })

    const unsubActiveProcesses = terminal.onActiveProcessesOpen(() => {
      set({ isActiveProcessesOpen: true })
    })

    const unsubSshStatus = ssh.onConnectionStatus((info) => {
      for (const entry of Array.from(get().sessionStores.values())) {
        const conn = entry.store.getState().connection
        if (conn && conn.id === info.id) {
          entry.store.setState({ connection: info })
        }
      }
    })

    // Handle initial workspace from CLI
    const initialPath = await getInitialWorkspace()
    if (initialPath) {
      const { activeView } = useNavigationStore.getState()
      const sessionId = activeView?.type === 'workspace' ? activeView.sessionId : null
      const sessionEntry = sessionId ? get().sessionStores.get(sessionId) : Array.from(get().sessionStores.values())[0]
      const connStatus = sessionEntry?.store.getState().connection?.status
      if (sessionEntry && connStatus !== ConnectionStatus.Connecting) {
        const { workspaces, addWorkspace, setActiveWorkspace } = sessionEntry.store.getState()
        let existingId: string | undefined
        for (const [wsId, e] of Array.from(workspaces.entries())) {
          if ((e.status === WorkspaceEntryStatus.Loaded || e.status === WorkspaceEntryStatus.OperationError) && e.data.path === initialPath) {
            existingId = wsId
            break
          }
        }
        if (existingId) {
          setActiveWorkspace(existingId)
        } else {
          addWorkspace(initialPath)
        }
      }
    }

    return () => {
      unsubSettings()
      unsubClose()
      unsubReady()
      unsubSync()
      unsubSshAutoConnected()
      unsubReconnected()
      unsubDisconnect()
      unsubActiveProcesses()
      unsubSshStatus()
    }
  },

  disconnectSession: (sessionId: string) => {
    set((state) => {
      const remaining = new Map(state.sessionStores)
      remaining.delete(sessionId)
      return { sessionStores: remaining }
    })
    // Clear navigation if it pointed to this session
    const { activeView } = useNavigationStore.getState()
    if (activeView && 'sessionId' in activeView && activeView.sessionId === sessionId) {
      const remainingIds = Array.from(get().sessionStores.keys())
      const nextSessionId = remainingIds[0]
      if (nextSessionId) {
        // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- nextSessionId from sessionStores.keys()
        const nextEntry = get().sessionStores.get(nextSessionId)!
        const workspaces = nextEntry.store.getState().workspaces
        const firstWsId = Array.from(workspaces.keys())[0]
        if (firstWsId) {
          useNavigationStore.getState().setActiveView({ type: 'workspace', workspaceId: firstWsId, sessionId: nextSessionId })
        } else {
          useNavigationStore.getState().setActiveView({ type: 'session', sessionId: nextSessionId })
        }
      }
    }
  },

  addRemoteSession: async (session: Session, connection: ConnectionInfo) => {
    console.log(`[renderer:app] addRemoteSession called: session=${session.id}, connection=${connection.id}, status=${connection.status}, workspaces=${String(session.workspaces.length)}`)
    // Reuse existing store (created eagerly in startRemoteConnect) or create new one
    const existingEntry = get().sessionStores.get(connection.id)
    let store: StoreApi<SessionState>
    if (existingEntry) {
      store = existingEntry.store
      // Update session ID and connection status on the existing store
      store.setState({ sessionId: session.id, connection })
      // Re-key from connection.id to session.id
      if (connection.id !== session.id) {
        set((state) => {
          const updated = new Map(state.sessionStores)
          updated.delete(connection.id)
          updated.set(session.id, { store })
          return { sessionStores: updated }
        })
      }
    } else {
      // Fallback: no prior connecting entry (e.g. --ssh auto-connect flow)
      store = getOrCreateSession(session.id, get, set, connection)
    }
    if (!useSessionNamesStore.getState().getName(session.id)) {
      const label = connection.target.type === 'remote'
        ? (connection.target.config.label || `${connection.target.config.user}@${connection.target.config.host}`)
        : session.id
      useSessionNamesStore.getState().setName(session.id, label)
    }
    console.log(`[renderer:app] Session store created/retrieved for session=${session.id}`)
    if (session.workspaces.length > 0) {
      console.log(`[renderer:app] Restoring ${String(session.workspaces.length)} workspaces for session=${session.id}`)
      await store.getState().handleRestore(session)
      // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- length > 0 checked above
      const firstWs = session.workspaces[0]!
      useNavigationStore.getState().setActiveView({ type: 'workspace', workspaceId: firstWs.id, sessionId: session.id })
    } else if (connection.target.type === 'remote') {
      const defaultPath = `/home/${connection.target.config.user}`
      console.log(`[renderer:app] No workspaces for session=${session.id}, creating default workspace at ${defaultPath}`)
      const workspaceId = store.getState().addWorkspace(defaultPath)
      useNavigationStore.getState().setActiveView({ type: 'workspace', workspaceId, sessionId: session.id })
    } else {
      console.log(`[renderer:app] No workspaces to restore for session=${session.id}`)
    }
  },

  startRemoteConnect: (config: SSHConnectionConfig) => {
    const connection: ConnectionInfo = { id: config.id, target: { type: 'remote', config }, status: ConnectionStatus.Connecting }
    getOrCreateSession(config.id, get, set, connection)
    if (!useSessionNamesStore.getState().getName(config.id)) {
      useSessionNamesStore.getState().setName(config.id, config.label || `${config.user}@${config.host}`)
    }
    useNavigationStore.getState().setActiveView({ type: 'session', sessionId: config.id })
  },

  setSessionError: (connectionId: string, error: string) => {
    // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- connectionId guaranteed to exist in sessionStores
    const entry = get().sessionStores.get(connectionId)!
    const conn = entry.store.getState().connection
    if (conn) {
      entry.store.setState({ connection: { ...conn, status: ConnectionStatus.Error, error } })
    }
  },

  removeSession: (id: string) => {
    set((state) => {
      const rest = new Map(state.sessionStores)
      rest.delete(id)
      return { sessionStores: rest }
    })
  },

}))

// Helper: get or create a session store
function getOrCreateSession(
  sessionId: string,
  get: () => AppState,
  set: (partial: Partial<AppState> | ((state: AppState) => Partial<AppState>)) => void,
  connection?: ConnectionInfo
): StoreApi<SessionState> {
  const { sessionStores, windowUuid, git, filesystem, exec, runActions, sessionApi, terminal, llm, github } = get()
  const existing = sessionStores.get(sessionId)
  if (existing) {
    console.log(`[renderer:app] getOrCreateSession: reusing existing session store for session=${sessionId}`)
    return existing.store
  }
  console.log(`[renderer:app] getOrCreateSession: creating new session store for session=${sessionId}, connection=${connection?.id ?? 'local'}`)
  const connId = connection?.id ?? 'local'
  const boundGit = createBoundGit(git, connId)
  const boundGithub = createBoundGitHub(github, connId)
  const boundFilesystem = createBoundFilesystem(filesystem, connId)
  const boundRunActions = createBoundRunActions(runActions, connId)
  const store = createSessionStore(
    { sessionId, windowUuid, connection },
    {
      git: boundGit,
      filesystem: boundFilesystem,
      exec,
      runActions: boundRunActions,
      sessionApi,
      terminal,
      getSettings: () => useSettingsStore.getState().settings,
      appRegistry: {
        get: (id: string) => get().applications.get(id),
        getDefaultApp: (appId?: string) => get().getDefaultApplication(appId),
      },
      github: boundGithub,
      llm: { analyzeTerminal: llm.analyzeTerminal, generateTitle: llm.generateTitle },
      setActivityTabState: (tabId, state) => { useActivityStateStore.getState().setTabState(tabId, state); },
    }
  )
  set((state) => ({
    sessionStores: new Map(state.sessionStores).set(sessionId, { store })
  }))
  console.log(`[renderer:app] getOrCreateSession: session store added to sessionStores, total sessions=${String(get().sessionStores.size)}`)
  return store
}

// Helper: dispose all workspaces in session stores matching a connectionId, then remove them
function disposeSessionForConnection(connectionId: string, get: () => AppState): void {
  const { sessionStores } = get()
  const toRemove: string[] = []

  for (const [sessionId, entry] of Array.from(sessionStores.entries())) {
    const conn = entry.store.getState().connection
    const connId = conn?.id ?? 'local'
    if (connId !== connectionId) continue

    console.log(`[renderer:app] Disposing session ${sessionId} for reconnecting connection ${connectionId}`)

    // Dispose all workspaces: cached terminals, tab refs, git controllers
    const workspaces = entry.store.getState().workspaces
    for (const [, wsEntry] of Array.from(workspaces.entries())) {
      if (wsEntry.status === WorkspaceEntryStatus.Loaded || wsEntry.status === WorkspaceEntryStatus.OperationError) {
        wsEntry.store.getState().gitController.getState().dispose()
        wsEntry.store.getState().disposeAllCachedTerminals()
        for (const tabId of Object.keys(wsEntry.data.appStates)) {
          const ref = wsEntry.store.getState().getTabRef(tabId)
          if (ref) ref.dispose()
        }
      }
    }

    toRemove.push(sessionId)
  }

  // Remove disposed session stores
  for (const sessionId of toRemove) {
    useAppStore.setState((state) => {
      const remaining = new Map(state.sessionStores)
      remaining.delete(sessionId)
      return { sessionStores: remaining }
    })
  }
}
