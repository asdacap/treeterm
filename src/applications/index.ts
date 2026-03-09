import { applicationRegistry } from '../renderer/registry/applicationRegistry'
import { terminalApplication, createTerminalApplication, createTerminalVariant } from './terminal/renderer'
import { filesystemApplication } from './filesystem/renderer'
import { claudeApplication } from './claude/renderer'
import { reviewApplication } from './review/renderer'
import type { TerminalInstance, Settings } from '../renderer/types'

let initialized = false

export function initializeApplications(): void {
  if (initialized) return

  applicationRegistry.register(terminalApplication)
  applicationRegistry.register(filesystemApplication)
  applicationRegistry.register(claudeApplication)
  applicationRegistry.register(reviewApplication)

  initialized = true
}

// Register dynamic terminal variants from settings and update base terminal
export function registerTerminalVariants(instances: TerminalInstance[], terminalSettings?: Settings['terminal']): void {
  // Re-register base terminal with updated startByDefault setting
  if (terminalSettings !== undefined) {
    const updatedTerminal = createTerminalApplication(terminalSettings.startByDefault)
    applicationRegistry.register(updatedTerminal)
  }

  // First, unregister any existing dynamic terminals
  const allApps = applicationRegistry.getAll()
  for (const app of allApps) {
    if (app.id.startsWith('terminal-')) {
      applicationRegistry.unregister(app.id)
    }
  }

  // Register new variants
  for (const instance of instances) {
    const variant = createTerminalVariant(instance)
    applicationRegistry.register(variant)
  }
}

export { terminalApplication, filesystemApplication, claudeApplication, reviewApplication }
