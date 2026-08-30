import { useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { useDismiss } from 'luvo/input/useDismiss';
import { placeMenu } from 'luvo/input/menu-place';

/** A menu opened at a point.
 *
 *  Both context menus were hand-rolled: the tab strip's dismissed on a click
 *  away and ignored Escape, the rail's dismissed on nothing at all, and neither
 *  checked whether the menu it placed fitted on the screen. One of them now,
 *  which also gives the keyboard somewhere to land — ⇧F10 opens this. */
export function ContextMenu({ at, onClose, children }: {
  at: { x: number; y: number };
  onClose: () => void;
  children: ReactNode;
}) {
  const ref = useDismiss<HTMLDivElement>(true, onClose);
  const [placed, setPlaced] = useState<{ left: number; top: number } | null>(null);
  const focused = useRef(false);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    setPlaced(placeMenu(at, { width, height }, { width: window.innerWidth, height: window.innerHeight }));
  }, [at, ref]);

  /* Focus lands once the menu has been placed — it is hidden until then, and
     focusing a hidden element does nothing. */
  useLayoutEffect(() => {
    if (!placed || focused.current) return;
    focused.current = true;
    ref.current?.querySelector<HTMLButtonElement>('.menu-item:not(:disabled)')?.focus();
  }, [placed, ref]);

  return (
    <div
      ref={ref}
      className="menu is-floating"
      role="menu"
      style={{ left: placed?.left ?? at.x, top: placed?.top ?? at.y, visibility: placed ? 'visible' : 'hidden' }}
    >
      {children}
    </div>
  );
}
