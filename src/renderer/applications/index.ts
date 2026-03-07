import { applicationRegistry } from '../registry/applicationRegistry'
import { terminalApplication } from './terminal'
import { filesystemApplication } from './filesystem'
import { claudeApplication } from './claude'

// Register all built-in applications
applicationRegistry.register(terminalApplication)
applicationRegistry.register(filesystemApplication)
applicationRegistry.register(claudeApplication)

export { terminalApplication, filesystemApplication, claudeApplication }
