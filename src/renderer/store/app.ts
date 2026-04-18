/* eslint-disable custom/no-string-literal-comparison -- TODO: migrate existing string-literal comparisons to enums */
import { create } from 'zustand'
import type { StoreApi } from 'zustand'
import { createSessionStore, WorkspaceEntryStatus } from './createSessionStore'
import type { SessionState, SessionEntry } from './createSessionStore'
import { getUnmergedSubWorkspaces } from './createSessionStore'
import { useSettingsStore } from './settings'
import { useKeybindingStore } from './keybinding'
import type { KeyEventTarget } from './keybinding'
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
  Platform, TerminalApi, SessionApi, AppApi, DaemonApi,
  RawFilesystemApi, ExecApi, SandboxApi, SettingsApi,
  TerminalInstance, AiHarnessInstance, CustomRunnerInstance,
  ConnectionInfo, SSHConnectionConfig, SSHApi, ClipboardApi
} from '../types'
import { createBoundFilesystem } from '../types'
import { createGitApi } from '../lib/gitClient'
import { createGitHubApi } from '../lib/githubClient'
import { createRunActionsApi } from '../lib/runActionsClient'
import { createWorktreeRegistryApi } from '../lib/worktreeRegistry'
import { createLlmClient } from '../lib/llmClient'
import { ConnectionStatus, ConnectionTargetType } from '../../shared/types'

export interface AppDeps {
  platform: Platform
  terminal: TerminalApi
  sessionApi: SessionApi
  settingsApi: SettingsApi
  appApi: AppApi
  daemon: DaemonApi
  filesystem: RawFilesystemApi
  exec: ExecApi
  sandbox: SandboxApi
  ssh: SSHApi
  clipboard: ClipboardApi
  selectFolder: () => Promise<string | null>
  getWindowUuid: () => Promise<string>
  getInitialWorkspace: () => Promise<string | null>
  openExternal: (url: string) => void
  getViewportSize: () => { width: number; height: number }
  keyEventTarget: KeyEventTarget
  isKeyDiagEnabled: () => boolean
}

interface AppState extends AppDeps {
  // Lifecycle
  windowUuid: string | null
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
  sessionApi: UNINITIALIZED,
  settingsApi: UNINITIALIZED,
  appApi: UNINITIALIZED,
  daemon: UNINITIALIZED,
  filesystem: UNINITIALIZED,
  exec: UNINITIALIZED,
  sandbox: UNINITIALIZED,
  ssh: UNINITIALIZED,
  clipboard: UNINITIALIZED,
  selectFolder: UNINITIALIZED,
  getWindowUuid: UNINITIALIZED,
  getInitialWorkspace: UNINITIALIZED,
  openExternal: UNINITIALIZED,
  getViewportSize: UNINITIALIZED,
  keyEventTarget: UNINITIALIZED,
  isKeyDiagEnabled: UNINITIALIZED,

  windowUuid: null,
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
      // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- guaranteed by .has() check
      return apps.get(appId)!
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
    const { terminal, sessionApi, settingsApi, appApi, ssh, getWindowUuid, getInitialWorkspace, keyEventTarget, isKeyDiagEnabled } = deps

    get().initializeApplications()
    useSettingsStore.getState().init(settingsApi, terminal.kill.bind(terminal))
    useKeybindingStore.getState().init(keyEventTarget)
    initKeyboardHealthMonitor(keyEventTarget, isKeyDiagEnabled)

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

    // Renderer drives local connection — mirrors sshConnect pattern
    const windowUuid = get().windowUuid
    if (windowUuid) {
      try {
        const { info, session } = await appApi.localConnect(windowUuid)
        console.log('[App] Local connection established, session:', session.id)
        const localSessionId = generateSessionId()
        const sessionStore = getOrCreateSession(localSessionId, get, set, info)
        if (!useSessionNamesStore.getState().getName(localSessionId)) {
          useSessionNamesStore.getState().setName(localSessionId, 'LOCAL')
        }
        if (session.workspaces.length > 0) {
          void sessionStore.getState().handleRestore(session)
          // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- length > 0 checked above
          const firstWs = session.workspaces[0]!
          useNavigationStore.getState().setActiveView({ type: 'workspace', workspaceId: firstWs.id, sessionId: localSessionId })
        }
      } catch (error) {
        console.error('[App] Failed to establish local connection:', error)
      }
    }

    const unsubSync = sessionApi.onSync((connectionId, session) => {
      console.log(`[App] Received session:sync from connection ${connectionId} with ${String(session.workspaces.length)} workspaces for session ${session.id}`)
      const found = findSessionByConnectionId(get, connectionId)
      if (!found) return
      void found.entry.store.getState().handleExternalUpdate(session)
    })

    const unsubSshAutoConnected = appApi.onSshAutoConnected((session, connection) => {
      console.log('[App] SSH auto-connected with session:', session.id, 'connection:', connection.id)
      void get().addRemoteSession(session, connection)
    })

    const unsubReconnected = appApi.onConnectionReconnected((session, connection) => {
      console.log('[App] Connection reconnected:', connection.id, 'session:', session.id)
      // Preserve session name before disposing old session
      const oldSession = findSessionByConnectionId(get, connection.id)
      const oldName = oldSession ? useSessionNamesStore.getState().getName(oldSession.key) : undefined
      // Dispose old session and recreate fresh
      disposeSessionForConnection(connection.id, get)
      const reconnSessionId = generateSessionId()
      const newStore = getOrCreateSession(reconnSessionId, get, set, connection)
      if (oldName) {
        useSessionNamesStore.getState().setName(reconnSessionId, oldName)
      }
      if (session.workspaces.length > 0) {
        void newStore.getState().handleRestore(session)
        // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- length > 0 checked above
        const firstWs = session.workspaces[0]!
        useNavigationStore.getState().setActiveView({ type: 'workspace', workspaceId: firstWs.id, sessionId: reconnSessionId })
      }
    })

    const unsubActiveProcesses = terminal.onActiveProcessesOpen(() => {
      set({ isActiveProcessesOpen: true })
    })

    const unsubSshStatus = ssh.onConnectionStatus((info) => {
      for (const entry of Array.from(get().sessionStores.values())) {
        const conn = entry.store.getState().connection
        if (conn.id === info.id) {
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
      const connStatus = sessionEntry?.store.getState().connection.status
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
      unsubSync()
      unsubSshAutoConnected()
      unsubReconnected()
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
    // Find existing store (created eagerly in startRemoteConnect) by connection ID
    const existing = findSessionByConnectionId(get, connection.id)
    let store: StoreApi<SessionState>
    let storeKey: string
    if (existing) {
      store = existing.entry.store
      storeKey = existing.key
      // Update connection status — no re-keying
      store.setState({ connection })
    } else {
      // Fallback: no prior connecting entry (e.g. --ssh auto-connect flow)
      storeKey = generateSessionId()
      store = getOrCreateSession(storeKey, get, set, connection)
    }
    if (!useSessionNamesStore.getState().getName(storeKey)) {
      const label = connection.target.type === ConnectionTargetType.Remote
        ? (connection.target.config.label || `${connection.target.config.user}@${connection.target.config.host}`)
        : storeKey
      useSessionNamesStore.getState().setName(storeKey, label)
    }
    console.log(`[renderer:app] Session store created/retrieved for session=${session.id}, storeKey=${storeKey}`)
    if (session.workspaces.length > 0) {
      console.log(`[renderer:app] Restoring ${String(session.workspaces.length)} workspaces for session=${session.id}`)
      await store.getState().handleRestore(session)
      // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- length > 0 checked above
      const firstWs = session.workspaces[0]!
      useNavigationStore.getState().setActiveView({ type: 'workspace', workspaceId: firstWs.id, sessionId: storeKey })
    } else if (connection.target.type === ConnectionTargetType.Remote) {
      const defaultPath = `/home/${connection.target.config.user}`
      console.log(`[renderer:app] No workspaces for session=${session.id}, creating default workspace at ${defaultPath}`)
      const workspaceId = store.getState().addWorkspace(defaultPath)
      useNavigationStore.getState().setActiveView({ type: 'workspace', workspaceId, sessionId: storeKey })
    } else {
      console.log(`[renderer:app] No workspaces to restore for session=${session.id}`)
    }
  },

  startRemoteConnect: (config: SSHConnectionConfig) => {
    const connection: ConnectionInfo = { id: config.id, target: { type: ConnectionTargetType.Remote, config }, status: ConnectionStatus.Connecting }
    const remoteSessionId = generateSessionId()
    getOrCreateSession(remoteSessionId, get, set, connection)
    if (!useSessionNamesStore.getState().getName(remoteSessionId)) {
      useSessionNamesStore.getState().setName(remoteSessionId, config.label || `${config.user}@${config.host}`)
    }
    useNavigationStore.getState().setActiveView({ type: 'session', sessionId: remoteSessionId })
  },

  setSessionError: (connectionId: string, error: string) => {
    const found = findSessionByConnectionId(get, connectionId)
    if (!found) return
    const conn = found.entry.store.getState().connection
    found.entry.store.setState({ connection: { ...conn, status: ConnectionStatus.Error, error } })
  },

  removeSession: (id: string) => {
    set((state) => {
      const rest = new Map(state.sessionStores)
      rest.delete(id)
      return { sessionStores: rest }
    })
  },

}))

function generateSessionId(): string {
  return `session-${String(Date.now())}-${Math.random().toString(36).slice(2, 9)}`
}

// Helper: find a session store by connection ID
function findSessionByConnectionId(get: () => AppState, connectionId: string): { key: string; entry: SessionEntry } | undefined {
  for (const [key, entry] of Array.from(get().sessionStores.entries())) {
    if (entry.store.getState().connection.id === connectionId) {
      return { key, entry }
    }
  }
  return undefined
}

// Helper: get or create a session store
function getOrCreateSession(
  sessionId: string,
  get: () => AppState,
  set: (partial: Partial<AppState> | ((state: AppState) => Partial<AppState>)) => void,
  connection: ConnectionInfo
): StoreApi<SessionState> {
  const { sessionStores, filesystem, exec, sessionApi, terminal } = get()
  const existing = sessionStores.get(sessionId)
  if (existing) {
    console.log(`[renderer:app] getOrCreateSession: reusing existing session store for session=${sessionId}`)
    return existing.store
  }
  console.log(`[renderer:app] getOrCreateSession: creating new session store for session=${sessionId}, connection=${connection.id}`)
  const connId = connection.id
  const boundFilesystem = createBoundFilesystem(filesystem, connId)
  const boundGit = createGitApi(exec, boundFilesystem, connId)
  const boundGithub = createGitHubApi(exec, get().settingsApi, connId)
  const boundRunActions = createRunActionsApi(boundFilesystem, terminal, connId)
  const boundWorktreeRegistry = createWorktreeRegistryApi(boundFilesystem, exec, connId)
  const store = createSessionStore(
    { sessionId, connection },
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
      worktreeRegistry: boundWorktreeRegistry,
      llm: createLlmClient(),
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
    const connId = conn.id
    if (connId !== connectionId) continue

    console.log(`[renderer:app] Disposing session ${sessionId} for reconnecting connection ${connectionId}`)

    // Dispose all workspaces: tab refs (includes cached terminals), git controllers
    const workspaces = entry.store.getState().workspaces
    for (const [, wsEntry] of Array.from(workspaces.entries())) {
      if (wsEntry.status === WorkspaceEntryStatus.Loaded || wsEntry.status === WorkspaceEntryStatus.OperationError) {
        wsEntry.store.getState().gitController.getState().dispose()
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
