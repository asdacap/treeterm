import { create } from 'zustand'

function clampContextMenuPosition(
  x: number,
  y: number,
  menuWidth = 160,
  menuHeight = 200
): { x: number; y: number } {
  const vw = window.innerWidth
  const vh = window.innerHeight
  return {
    x: Math.min(x, vw - menuWidth),
    y: Math.min(y, vh - menuHeight),
  }
}

interface ContextMenuState {
  activeMenuId: string | null
  position: { x: number; y: number }
  open: (menuId: string, x: number, y: number, menuWidth?: number, menuHeight?: number) => void
  close: () => void
}

export const useContextMenuStore = create<ContextMenuState>()((set) => ({
  activeMenuId: null,
  position: { x: 0, y: 0 },
  open: (menuId, x, y, menuWidth, menuHeight) =>
    { set({ activeMenuId: menuId, position: clampContextMenuPosition(x, y, menuWidth, menuHeight) }); },
  close: () => { set({ activeMenuId: null }); },
}))

export function handleClickOutside(e: { target: EventTarget | null }): void {
  const { activeMenuId } = useContextMenuStore.getState()
  if (!activeMenuId) return
  const target = e.target as HTMLElement
  if (target.closest('.context-menu')) return
  useContextMenuStore.getState().close()
}

interface ClickListenable {
  addEventListener(type: string, handler: (e: MouseEvent) => void, capture: boolean): void
}

export function installClickListener(doc: ClickListenable): void {
  doc.addEventListener('click', handleClickOutside as (e: MouseEvent) => void, true)
}

// Set up once at module load (no useEffect)
if (typeof document !== 'undefined') {
  installClickListener(document)
}
