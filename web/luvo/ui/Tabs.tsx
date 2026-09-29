import { useId, useRef, type ReactNode } from 'react';
import { nextTabIndex } from 'luvo/input/tab-keys';
import { tabIds } from 'luvo/ui/tab-ids';

export interface TabItem<K extends string> {
  key: K;
  label: ReactNode;
  title?: string;
}

/** Every tab strip in the workbench, once.
 *
 *  There were five of them, each a row of plain buttons: no `role`, no
 *  `aria-selected`, and no arrows — a keyboard reached them one Tab stop at a
 *  time and a screen reader was told nothing about what they were. One strip
 *  now, with the roving tabindex the pattern asks for; `children` is whatever a
 *  caller keeps on the same line, a spacer or a second group. */
export function Tabs<K extends string>({ items, value, onChange, children, className, tabClassName, label, id }: {
  items: TabItem<K>[];
  value: K;
  onChange: (key: K) => void;
  children?: ReactNode;
  className?: string;
  /** What this strip is, for a reader who cannot see where it sits. Four
      strips in one window, all announced as "tab list" and nothing else. */
  label?: string;
  /** Added to every tab in this strip — the environment manager's two share the
      row equally, which is a property of that strip and not of tabs. */
  tabClassName?: string;
  id?: string;
}) {
  const strip = useRef<HTMLElement>(null);
  const generated = useId();
  const base = id ?? generated;

  const onKeyDown = (e: React.KeyboardEvent) => {
    const at = items.findIndex(i => i.key === value);
    const next = nextTabIndex(at < 0 ? 0 : at, items.length, e.key);
    if (next === null) return;
    e.preventDefault();
    onChange(items[next].key);
    strip.current?.querySelectorAll<HTMLButtonElement>('[role="tab"]')[next]?.focus();
  };

  return (
    <nav
      ref={strip}
      className={className ? `tabs ${className}` : 'tabs'}
      role="tablist"
      aria-label={label}
      onKeyDown={onKeyDown}
    >
      {items.map(item => (
        <button
          key={item.key}
          id={tabIds(base, item.key).tab}
          role="tab"
          aria-selected={value === item.key}
          aria-controls={tabIds(base, item.key).panel}
          tabIndex={value === item.key ? 0 : -1}
          title={item.title}
          className={`tab${tabClassName ? ` ${tabClassName}` : ''}${value === item.key ? ' is-on' : ''}`}
          onClick={() => onChange(item.key)}
        >
          {item.label}
        </button>
      ))}
      {children}
    </nav>
  );
}
