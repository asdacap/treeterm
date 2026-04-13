import PierreDiffsWorker from '@pierre/diffs/worker/worker.js?worker'
import { registerCustomTheme } from '@pierre/diffs'

export function createDiffsWorker(): Worker {
  return new PierreDiffsWorker()
}

registerCustomTheme('treeterm-dark', async (): Promise<import('shiki').ThemeRegistration> => {
  const { bundledThemes } = await import('shiki')
  const themeModule = await bundledThemes['dark-plus']()
  if ('default' in themeModule) {
    return themeModule.default
  }
  return themeModule
})
