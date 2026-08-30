import { useLayoutEffect, useRef, useState, type ReactNode, type RefObject } from 'react';
import { createPortal } from 'react-dom';
import { placeMenu } from 'luvo/input/menu-place';

export interface PopoverProps {
  open: boolean;
  anchor: RefObject<HTMLElement | null>;
  align?: 'start' | 'end';
  /** As wide as the control it hangs from — a picker whose menu is the field. */
  matchWidth?: boolean;
  className?: string;
  children: ReactNode;
}

export function Popover({ open, ...rest }: PopoverProps) {
  if (!open) return null;
  return <Placed {...rest} />;
}

function Placed({ anchor, align = 'start', matchWidth = false, className, children }: Omit<PopoverProps, 'open'>) {
  const box = useRef<HTMLDivElement>(null);
  const [at, setAt] = useState<{ left: number; top: number; width?: number } | null>(null);

  useLayoutEffect(() => {
    const place = () => {
      const trigger = anchor.current?.getBoundingClientRect();
      if (!trigger) return;
      const size = box.current?.getBoundingClientRect();
      const width = matchWidth ? trigger.width : size?.width ?? 0;
      const height = size?.height ?? 0;
      const x = align === 'end' ? trigger.right - width : trigger.left;
      const spot = placeMenu(
        { x, y: trigger.bottom + 2 },
        { width, height },
        { width: window.innerWidth, height: window.innerHeight },
      );
      setAt(matchWidth ? { ...spot, width } : spot);
    };
    place();
    window.addEventListener('resize', place);
    window.addEventListener('scroll', place, true);
    return () => {
      window.removeEventListener('resize', place);
      window.removeEventListener('scroll', place, true);
    };
  }, [anchor, align, matchWidth, children]);

  return createPortal(
    <div
      ref={box}
      className={className ? `popover ${className}` : 'popover'}
      data-popover=""
      style={{
        left: `${at?.left ?? 0}px`,
        top: `${at?.top ?? 0}px`,
        width: at?.width !== undefined ? `${at.width}px` : undefined,
        visibility: at ? 'visible' : 'hidden',
      }}
    >
      {children}
    </div>,
    document.body,
  );
}
