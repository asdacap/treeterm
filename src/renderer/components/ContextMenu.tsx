interface ContextMenuProps {
  menuId: string
  activeMenuId: string | null
  position: { x: number; y: number }
  children: React.ReactNode
}

export default function ContextMenu({ menuId, activeMenuId, position, children }: ContextMenuProps) {
  if (activeMenuId !== menuId) return null
  return (
    <div className="context-menu" style={{ top: position.y, left: position.x }}>
      {children}
    </div>
  )
}
