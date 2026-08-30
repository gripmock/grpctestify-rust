import { useRef, type ReactNode } from 'react';
import { nextTabIndex } from 'luvo/input/tab-keys';

export interface SegOption<V extends string> {
  value: V;
  label: ReactNode;
  title?: string;
  disabled?: boolean;
}

/** Every segmented choice in the workbench, once.
 *
 *  There were fifteen of them, each a row of plain buttons carrying the choice
 *  in a class name: a screen reader heard five unrelated buttons and was told
 *  nothing about which one was on, and a keyboard walked them one Tab stop at a
 *  time. One control now — a radio group with the roving tabindex the pattern
 *  asks for, arrows that move and choose. */
export function Seg<V extends string>({ label, value, options, onChange, className }: {
  label: string;
  value: V | null;
  options: SegOption<V>[];
  onChange: (value: V) => void;
  className?: string;
}) {
  const group = useRef<HTMLDivElement>(null);

  const onKeyDown = (e: React.KeyboardEvent) => {
    const at = options.findIndex(o => o.value === value);
    const next = nextTabIndex(at < 0 ? 0 : at, options.length, e.key);
    if (next === null) return;
    e.preventDefault();
    const option = options[next];
    if (option.disabled) return;
    onChange(option.value);
    group.current?.querySelectorAll<HTMLButtonElement>('[role="radio"]')[next]?.focus();
  };

  /* Nothing chosen still leaves one way in: a group where every option is
     `tabIndex={-1}` cannot be reached by the keyboard at all. */
  const focused = options.findIndex(o => o.value === value);
  const stop = focused < 0 ? options.findIndex(o => !o.disabled) : focused;

  return (
    <div
      ref={group}
      className={className ? `seg ${className}` : 'seg'}
      role="radiogroup"
      aria-label={label}
      onKeyDown={onKeyDown}
    >
      {options.map((option, at) => (
        <button
          key={option.value}
          type="button"
          role="radio"
          aria-checked={option.value === value}
          tabIndex={at === stop ? 0 : -1}
          disabled={option.disabled}
          title={option.title}
          className={option.value === value ? 'is-on' : undefined}
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}
