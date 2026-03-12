import type { Application } from '../types'

class ApplicationRegistry {
  private applications = new Map<string, Application>()

  register(application: Application): void {
    this.applications.set(application.id, application)
  }

  unregister(id: string): void {
    this.applications.delete(id)
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

  getDefaultApps(): Application[] {
    return this.getAll().filter((app) => app.isDefault)
  }

  // Get a single default app by ID, or fall back to the first available app
  getDefaultApp(appId?: string): Application | null {
    if (appId) {
      const app = this.applications.get(appId)
      if (app) return app
    }
    // Fall back to first available app
    const allApps = this.getAll()
    return allApps.length > 0 ? allApps[0] : null
  }
}

export const applicationRegistry = new ApplicationRegistry()
