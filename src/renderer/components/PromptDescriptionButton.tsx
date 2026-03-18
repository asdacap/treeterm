import { useTerminalApi } from '../contexts/TerminalApiContext'

interface PromptDescriptionButtonProps {
  description: string
  ptyId: string | undefined
  onDismiss: () => void
}

export function PromptDescriptionButton({ description, ptyId, onDismiss }: PromptDescriptionButtonProps): JSX.Element | null {
  const terminal = useTerminalApi()

  if (!ptyId) return null

  const handlePrompt = () => {
    terminal.write(ptyId, description + '\r')
    onDismiss()
  }

  return (
    <div className="prompt-description-button">
      <button onClick={handlePrompt} title="Send description to AI agent">
        Prompt Description
      </button>
      <button onClick={onDismiss} title="Skip sending description">
        Skip
      </button>
    </div>
  )
}
