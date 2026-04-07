import { useStore } from 'zustand'
import type { WorkspaceStore } from '../types'

interface PromptDescriptionButtonProps {
  description: string
  workspace: WorkspaceStore
  onDismiss: () => void
}

export function PromptDescriptionButton({ description, workspace, onDismiss }: PromptDescriptionButtonProps): JSX.Element | null {
  const { promptHarness } = useStore(workspace)

  const handlePrompt = () => {
    void promptHarness(description)
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
