import type { StoreApi } from 'zustand'
import { useStore } from 'zustand'
import type { WorkspaceState } from '../store/createWorkspaceStore'

interface PromptDescriptionButtonProps {
  description: string
  workspaceStore: StoreApi<WorkspaceState>
  workspaceId: string
  onDismiss: () => void
}

export function PromptDescriptionButton({ description, workspaceStore, workspaceId, onDismiss }: PromptDescriptionButtonProps): JSX.Element | null {
  const promptHarness = useStore(workspaceStore, (state) => state.promptHarness)

  const handlePrompt = () => {
    promptHarness(workspaceId, description)
    onDismiss()
  }

  return (
    <span className="prompt-description-button">
      <button onClick={handlePrompt} title="Send description to AI agent">
        Prompt Description
      </button>
      <button onClick={onDismiss} title="Skip sending description">
        Skip
      </button>
    </span>
  )
}
