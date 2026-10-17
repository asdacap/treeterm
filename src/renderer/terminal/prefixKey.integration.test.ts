// @vitest-environment jsdom
import { expect, it, vi } from 'vitest'
import { createGhosttyEngine } from './ghosttyEngine'
import { createXtermEngine } from './xtermEngine'
import { PrefixModeState, useKeybindingStore } from '../store/keybinding'

vi.mock('../utils/loadWebglRenderer', () => ({ loadWebglRenderer: vi.fn() }))
vi.mock('../store/settings', () => ({
  useSettingsStore: {
    getState: () => ({ settings: {
      prefixMode: { enabled: true, prefixKey: 'Control+B', timeout: 1500 },
      keybindings: {},
    } }),
  },
}))

it.each([
  { name: 'Ghostty', createEngine: createGhosttyEngine },
  { name: 'xterm', createEngine: createXtermEngine },
])('$name receives exactly one Ctrl-B for two prefix presses', async ({ createEngine }) => {
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue({
    measureText: (text: string) => ({ width: text.length * 8, actualBoundingBoxAscent: 12, actualBoundingBoxDescent: 4 }),
    fillRect() {}, clearRect() {}, fillText() {}, save() {}, restore() {}, beginPath() {},
    rect() {}, clip() {}, translate() {}, scale() {}, setTransform() {}, drawImage() {},
    moveTo() {}, lineTo() {}, stroke() {}, closePath() {},
    createLinearGradient: () => ({ addColorStop() {} }),
  } as unknown as CanvasRenderingContext2D)
  vi.stubGlobal('matchMedia', vi.fn(() => ({
    matches: false, addListener() {}, removeListener() {},
    addEventListener() {}, removeEventListener() {},
  })))
  const engine = await createEngine({
    fontSize: 14, fontFamily: 'monospace', cursorStyle: 'block', cursorBlink: false,
    themeBackground: '#1e1e1e', allowOsc52Clipboard: false, scrollback: 100,
    openExternal: vi.fn(), writeClipboardText: vi.fn(), label: 'Prefix test',
  })
  const container = document.createElement('div')
  document.body.appendChild(container)
  const onData = vi.fn()
  const subscription = engine.onData(onData)
  try {
    engine.attach(container)
    useKeybindingStore.getState().deactivate()
    useKeybindingStore.getState().init(window)
    const input = container.querySelector<HTMLElement>('textarea, [contenteditable="true"]')
    if (!input) throw new Error('Terminal input missing')
    input.focus()
    const press = (): void => {
      input.dispatchEvent(new KeyboardEvent('keydown', {
        key: 'b', code: 'KeyB', keyCode: 66, ctrlKey: true, bubbles: true, cancelable: true,
      }))
      input.dispatchEvent(new KeyboardEvent('keyup', {
        key: 'b', code: 'KeyB', keyCode: 66, ctrlKey: true, bubbles: true,
      }))
    }
    press()
    expect(onData).not.toHaveBeenCalled()
    press()
    expect(onData).toHaveBeenCalledExactlyOnceWith('\x02')
    expect(useKeybindingStore.getState().prefixState).toBe(PrefixModeState.Idle)
  } finally {
    useKeybindingStore.getState().dispose()
    subscription.dispose()
    engine.dispose()
    container.remove()
    vi.restoreAllMocks()
    vi.unstubAllGlobals()
  }
})
