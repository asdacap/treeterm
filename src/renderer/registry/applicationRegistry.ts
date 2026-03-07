import type { Application } from '../types'

class ApplicationRegistry {
  private applications = new Map<string, Application>()

  register(application: Application): void {
    this.applications.set(application.id, application)
  }

  get(id: string): Application | undefined {
    return this.applications.get(id)
  }

  getAll(): Application[] {
    return Array.from(this.applications.values())
  }

  getMenuItems(): Application[] {
    return this.getAll().filter((app) => app.showInNewTabMenu)
  }
}

export const applicationRegistry = new ApplicationRegistry()
