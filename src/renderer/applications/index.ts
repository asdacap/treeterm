import { applicationRegistry } from '../registry/applicationRegistry'
import { terminalApplication, createTerminalVariant } from './terminal'
import { filesystemApplication } from './filesystem'
import { claudeApplication } from './claude'
import type { TerminalInstance } from '../types'

// Register all built-in applications
applicationRegistry.register(terminalApplication)
applicationRegistry.register(filesystemApplication)
applicationRegistry.register(claudeApplication)

// Register dynamic terminal variants from settings
export function registerTerminalVariants(instances: TerminalInstance[]): void {
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

export { terminalApplication, filesystemApplication, claudeApplication }
