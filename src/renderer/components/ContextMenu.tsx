import { useContextMenuStore } from '../store/contextMenu'

interface ContextMenuProps {
  menuId: string
  children: React.ReactNode
}

export default function ContextMenu({ menuId, children }: ContextMenuProps) {
  const activeMenuId = useContextMenuStore((s) => s.activeMenuId)
  const position = useContextMenuStore((s) => s.position)
  if (activeMenuId !== menuId) return null
  return (
    <div className="context-menu" style={{ top: position.y, left: position.x }}>
      {children}
    </div>
  )
}
