import type { DingPlayer } from '../audio/ding'
import { createWorkspaceAttentionCoordinator, type WorkspaceAttentionCoordinatorDeps } from './workspaceAttentionCoordinator'

/** Renderer lifetime, not React lifetime: StrictMode/sidebar churn cannot replay sounds. */
export function startWorkspaceNotifications(
  deps: Omit<WorkspaceAttentionCoordinatorDeps, 'playDing'>,
  player: DingPlayer,
): () => void {
  const stop = createWorkspaceAttentionCoordinator({ ...deps, playDing: player.playDing })
  let disposed = false
  return (): void => {
    if (disposed) return
    disposed = true
    stop()
    void player.dispose().catch(deps.reportError)
  }
}
