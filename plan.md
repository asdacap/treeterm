# Treeterm

Treeterm aims to be an alternative for tmux when using AI agents.

So I use claude via terminal. AI agents to some extent is a bit slow, but you can have multiple of them at the same time. This mean to improve productivity, we need to have multiple instance of claude doing things. 

This is not particularly new. I have multiple copies of nethermind repo because a lot of the time I need to run one and wait for it to complete. So in the mean time, I open another tmux instance on another repo.

With claude, this need expand even further as it kinda take some time for a task to complete. A lot of the time, I would open another git worktree so that I would open another claude instance within that worktree. Claude is not very consistent with worktree though. 

This shows another issue with claude, it ask a lot of question. I would really like to just approve everything but I really dont want it to have write access to the rest of my system. So I wish there is something that also isolate the inner terminal so that I would just let claude do anything it need to do as long as it mutates within the worktree.

This is where this project inspires to be. Instead of an AI editor, it is a terminal manager mainly with hierarcical terminal instance. That said, it can also eventually become something more feature complete but rather than integrating the LLM within it, it mainly calls other code editor or plain terminal, or just me manually coding things.

## Design

Treeterm is an electron app, because I dont wanna deal with desktop issues. For now it has two pane, left smaller pane and right big workspace. 

The left pane is a tree of workspace. Each workspace is essentially a terminal window, but I want it to have a tab so that I can open multiple terminal in a workspace. In another word, a workspace a configuration to a current directory. Preferably the directory is a git repository. So the terminal simply open with that git repository as the curret directory.

A workspace can have a child workspace. When a child workspace is created, it create a git worktree of that directory and that the child workspace inner directory would be that directory. The idea is that a child task for the agent is handled within that workspace. In the future, I want to run the agents completely fully autonomously without making me approve permission by having the terminal for this workspace run within a sanbox or a linux namespace.

Another iadea is that instead of openinng a terminal, it opens a diff so that we can review the changed items and diff it with the parent workspace.

When a child workspace is closed, it may be merged with its parent workspace, basically a git merge. So this is a bit tricky to get right. Ideally there should be an interface that shows a diff. Optionally also, we can just abort merge and remove the worktree.

## Architecture

### Tech Stack
- **Electron** - Cross-platform desktop app
- **xterm.js** - Terminal emulation in the browser
- **node-pty** - Pseudoterminal bindings for Node.js
- **React** (or Vue/Svelte) - UI framework for the panes and tree view
- **simple-git** - Git operations (worktree creation, merging, diff)

### Component Structure
```
┌─────────────────────────────────────────────────────────┐
│                      Main Window                        │
├──────────────┬──────────────────────────────────────────┤
│              │                                          │
│  Tree Pane   │           Workspace Pane                 │
│  (200-300px) │                                          │
│              │  ┌────────────────────────────────────┐  │
│  ▼ Repo A    │  │  Tab Bar (Terminal 1 | Terminal 2) │  │
│    ├─ task-1 │  ├────────────────────────────────────┤  │
│    └─ task-2 │  │                                    │  │
│  ▶ Repo B    │  │         Terminal Content           │  │
│              │  │           (xterm.js)               │  │
│              │  │                                    │  │
│              │  └────────────────────────────────────┘  │
│              │                                          │
└──────────────┴──────────────────────────────────────────┘
```

### Data Model
```typescript
interface Workspace {
  id: string;
  name: string;
  path: string;                    // absolute path to directory
  gitBranch: string;               // current branch
  parentId: string | null;         // null for root workspaces
  children: string[];              // child workspace IDs
  worktreePath: string | null;     // git worktree path if child
  terminals: Terminal[];
  status: 'active' | 'merged' | 'abandoned';
}

interface Terminal {
  id: string;
  workspaceId: string;
  pid: number;
  title: string;
  isActive: boolean;
}

interface AppState {
  workspaces: Map<string, Workspace>;
  activeWorkspaceId: string | null;
  activeTerminalId: string | null;
}
```

## Core Features

### 1. Workspace Tree Management
- Add root workspace by selecting a git repository
- Right-click context menu: "New Child Workspace" creates a git worktree
- Drag-and-drop to reorganize (if needed)
- Visual indicators for workspace status (has changes, merge conflicts, etc.)
- Collapse/expand tree nodes

### 2. Git Worktree Integration
When creating a child workspace:
```bash
# Create a new branch and worktree
git worktree add ../repo-task-1 -b task-1-branch

# Or use existing branch
git worktree add ../repo-task-1 existing-branch
```

Naming convention for worktrees:
- `{repo-name}-{workspace-name}` stored alongside the main repo
- Or use a configurable `.treeterm/worktrees/` directory

### 3. Terminal Tabs per Workspace
- Each workspace has its own set of terminal tabs
- Terminals persist across workspace switches
- Keyboard shortcuts: Cmd+T (new tab), Cmd+W (close tab), Cmd+1-9 (switch tab)
- Terminal state preserved when switching workspaces

### 4. Diff/Merge Interface
When closing a child workspace, present options:
1. **Merge** - Open a diff view showing changes vs parent
   - Use `git diff parent-branch...child-branch`
   - Show file tree of changed files
   - Allow per-file accept/reject
   - Perform `git merge` on accept
2. **Squash Merge** - Combine all commits into one
3. **Abandon** - Delete worktree and branch without merging
4. **Keep Open** - Cancel the close operation

### 5. Sandbox/Isolation (Future)
Options for isolating child workspaces:

**Linux Namespaces (Linux only):**
```bash
# Run terminal in a new namespace with limited access
unshare --mount --pid --fork --map-root-user
```

**Docker Container:**
```bash
# Mount worktree into a container
docker run -it -v /path/to/worktree:/workspace ubuntu bash
```

**macOS Sandbox:**
- Use `sandbox-exec` with a custom profile
- Limit file system access to the worktree directory only
- Block network if needed

**Permissions Model:**
```typescript
interface SandboxConfig {
  fileSystemAccess: 'worktree-only' | 'read-only-parent' | 'full';
  networkAccess: boolean;
  allowedPaths: string[];   // additional paths to whitelist
  environment: Record<string, string>;
}
```

## User Workflows

### Workflow 1: Parallel Agent Tasks
1. Open main repo as root workspace
2. Create child workspace "feature-auth" → creates worktree
3. Create child workspace "feature-api" → creates worktree
4. Run Claude in each child workspace terminal
5. Review diffs as each completes
6. Merge successful ones, abandon failed ones

### Workflow 2: Iterative Refinement
1. Agent completes task in child workspace
2. Review diff, find issues
3. Create grandchild workspace for refinement
4. Merge grandchild → child → parent when satisfied

### Workflow 3: Safe Experimentation
1. Create sandboxed child workspace
2. Let agent run with full autonomy (auto-approve all)
3. Review all changes via diff before any merge
4. Either accept all or discard entirely

## Implementation Phases

### Phase 1: Basic Shell
- Electron window with split panes
- Tree view with manual workspace management (no git integration yet)
- Single terminal per workspace using xterm.js + node-pty
- Workspace persistence (save/load state)

### Phase 2: Git Worktrees
- Git repository detection
- Create child workspaces as worktrees
- Delete worktrees on workspace removal
- Basic branch display

### Phase 3: Terminal Tabs
- Multiple terminals per workspace
- Tab bar UI
- Keyboard shortcuts
- Terminal title detection

### Phase 4: Diff/Merge
- Diff view using a library like `diff2html` or Monaco diff editor
- Merge operations with conflict resolution
- Workspace status indicators

### Phase 5: Sandboxing
- Configuration UI for sandbox settings
- Platform-specific sandbox implementations
- Auto-approve mode for sandboxed workspaces

## Configuration

```json
{
  "worktreeDirectory": "adjacent",  // or "nested" or custom path
  "defaultShell": "/bin/zsh",
  "theme": "dark",
  "keyBindings": {
    "newTerminal": "Cmd+T",
    "closeTerminal": "Cmd+W",
    "newChildWorkspace": "Cmd+Shift+N"
  },
  "sandbox": {
    "defaultEnabled": false,
    "autoApproveInSandbox": true
  }
}
```

## Challenges & Considerations

1. **Worktree Cleanup** - Ensure worktrees are properly removed when workspaces are deleted, even on crash
2. **Merge Conflicts** - Need a good UI for resolving conflicts; could integrate with external merge tools
3. **Performance** - Many terminals can consume memory; consider lazy loading or pty pooling
4. **Cross-Platform Sandboxing** - Different approaches needed for macOS, Linux, Windows
5. **Agent Detection** - Could auto-detect when Claude/AI is running and offer sandbox mode
6. **Git Submodules** - Handle repos with submodules gracefully in worktrees

## Future Ideas

- **Session Recording** - Record terminal sessions for replay/review
- **AI Integration** - Built-in Claude API for automated task dispatch to child workspaces
- **Remote Workspaces** - SSH into remote machines, create worktrees there
- **Workspace Templates** - Pre-configured workspace setups for common workflows
- **Notifications** - Alert when a child workspace agent completes or needs input
- **Metrics Dashboard** - Track agent success rates, time spent, merge rates
