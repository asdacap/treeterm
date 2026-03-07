import { applicationRegistry } from '../registry/applicationRegistry'
import { terminalApplication } from './terminal'
import { filesystemApplication } from './filesystem'

// Register all built-in applications
applicationRegistry.register(terminalApplication)
applicationRegistry.register(filesystemApplication)

export { terminalApplication, filesystemApplication }
