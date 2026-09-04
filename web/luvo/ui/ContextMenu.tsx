import { useLayoutEffect, useState, type ReactNode } from 'react';
import { useDismiss } from 'luvo/input/useDismiss';
import { useMenuKeys } from 'luvo/input/useMenuKeys';
import { placeMenu } from 'luvo/input/menu-place';

/** A menu opened at a point.
 *
 *  Both context menus were hand-rolled: the tab strip's dismissed on a click
 *  away and ignored Escape, the rail's dismissed on nothing at all, and neither
 *  checked whether the menu it placed fitted on the screen. One of them now,
 *  which also gives the keyboard somewhere to land — ⇧F10 opens this. */
export function ContextMenu({ at, onClose, className, label, children }: {
  at: { x: number; y: number };
  onClose: () => void;
  className?: string;
  label?: string;
  children: ReactNode;
}) {
  const ref = useDismiss<HTMLDivElement>(true, onClose);
  const [menuRef, onMenuKeys] = useMenuKeys<HTMLDivElement>(true, onClose);
  const [placed, setPlaced] = useState<{ left: number; top: number } | null>(null);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    setPlaced(placeMenu(at, { width, height }, { width: window.innerWidth, height: window.innerHeight }));
  }, [at, ref]);

  return (
    <div
      ref={node => { ref.current = node; menuRef.current = node; }}
      className={className ? `menu is-floating ${className}` : 'menu is-floating'}
      role="menu"
      aria-label={label}
      onKeyDown={onMenuKeys}
      style={{ left: placed?.left ?? at.x, top: placed?.top ?? at.y, visibility: placed ? 'visible' : 'hidden' }}
    >
      {children}
    </div>
  );
}
