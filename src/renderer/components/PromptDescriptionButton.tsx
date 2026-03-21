import { useStore } from 'zustand'
import type { WorkspaceHandle } from '../types'

interface PromptDescriptionButtonProps {
  description: string
  workspace: WorkspaceHandle
  onDismiss: () => void
}

export function PromptDescriptionButton({ description, workspace, onDismiss }: PromptDescriptionButtonProps): JSX.Element | null {
  const { promptHarness } = useStore(workspace)

  const handlePrompt = () => {
    promptHarness(description)
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
