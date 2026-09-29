import type React from 'react';

/** The one splitter. Three of them were hand-rolled, each `role="separator"`
 *  with nothing but a mousedown behind it — a promise to assistive tech that
 *  the keyboard could move it, kept by none of them.
 *
 *  Dragging stays with the caller, because each split does its own arithmetic;
 *  what lives here is the part that was missing everywhere: arrows move it,
 *  Shift moves it faster, Home and End take it to its limits, and the value it
 *  is at is announced. */
export function Splitter({
  className,
  orientation,
  value,
  min,
  max,
  step = 16,
  invert = false,
  label,
  title,
  onValue,
  onMouseDown,
  onDoubleClick,
}: {
  className: string;
  orientation: 'vertical' | 'horizontal';
  value: number;
  min: number;
  max: number;
  /** How far one arrow press moves it. */
  step?: number;
  /** True when growing means moving up or left — the drawer grows upward. */
  invert?: boolean;
  label: string;
  title?: string;
  onValue: (next: number) => void;
  onMouseDown?: (e: React.MouseEvent) => void;
  onDoubleClick?: () => void;
}) {
  const clamp = (n: number) => Math.min(max, Math.max(min, Math.round(n)));

  return (
    <div
      className={className}
      role="separator"
      aria-orientation={orientation}
      aria-label={label}
      aria-valuenow={Math.round(value)}
      aria-valuemin={min}
      aria-valuemax={max}
      tabIndex={0}
      title={title}
      onMouseDown={onMouseDown}
      onDoubleClick={onDoubleClick}
      onKeyDown={e => {
        const by = (e.shiftKey ? step * 4 : step) * (invert ? -1 : 1);
        const keys: Record<string, number | 'min' | 'max'> = {
          ArrowLeft: -by, ArrowRight: by, ArrowUp: -by, ArrowDown: by,
          Home: 'min', End: 'max',
        };
        const move = keys[e.key];
        if (move === undefined) return;
        e.preventDefault();
        onValue(move === 'min' ? min : move === 'max' ? max : clamp(value + move));
      }}
    />
  );
}
