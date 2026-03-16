import { applicationRegistry } from '../renderer/registry/applicationRegistry'
import { createTerminalApplication, createTerminalVariant } from './terminal/renderer'
import { filesystemApplication } from './filesystem/renderer'
import { createAiHarnessVariant } from './aiHarness/renderer'
import { reviewApplication } from './review/renderer'
import { editorApplication } from './editor/renderer'
import type { TerminalInstance, AiHarnessInstance, Settings, TerminalApi } from '../renderer/types'

type TerminalDeps = { terminal: Pick<TerminalApi, 'kill'> }

let initialized = false

export function initializeApplications(deps: TerminalDeps): void {
  if (initialized) return

  applicationRegistry.register(createTerminalApplication(true, deps))
  applicationRegistry.register(filesystemApplication)
  // NOTE: AI Harness variants are registered dynamically from settings
  applicationRegistry.register(reviewApplication)
  applicationRegistry.register(editorApplication)

  initialized = true
}

// Register dynamic terminal variants from settings and update base terminal
export function registerTerminalVariants(instances: TerminalInstance[], terminalSettings: Settings['terminal'] | undefined, terminalKill: ((id: string) => void) | null): void {
  if (!terminalKill) return

  const deps: TerminalDeps = { terminal: { kill: terminalKill } }

  // Re-register base terminal with updated startByDefault setting
  if (terminalSettings !== undefined) {
    const updatedTerminal = createTerminalApplication(terminalSettings.startByDefault, deps)
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
    const variant = createTerminalVariant(instance, deps)
    applicationRegistry.register(variant)
  }
}

// Register dynamic AI Harness variants from settings
export function registerAiHarnessVariants(instances: AiHarnessInstance[], terminalKill: ((id: string) => void) | null): void {
  if (!terminalKill) return

  const deps: TerminalDeps = { terminal: { kill: terminalKill } }

  // First, unregister any existing dynamic AI Harness apps
  const allApps = applicationRegistry.getAll()
  for (const app of allApps) {
    if (app.id.startsWith('aiharness-')) {
      applicationRegistry.unregister(app.id)
    }
  }

  // Register new variants
  for (const instance of instances) {
    const variant = createAiHarnessVariant(instance, deps)
    applicationRegistry.register(variant)
  }
}

export { filesystemApplication, reviewApplication, createAiHarnessVariant }
