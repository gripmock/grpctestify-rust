export interface HotkeyDef {
  key: string;
  ctrl?: boolean;
  shift?: boolean;
  alt?: boolean;
  meta?: boolean;
  category: 'execution' | 'run' | 'file' | 'tabs' | 'navigation' | 'general';
  description: string;
  /** What the help table prints instead of the key, when one row stands for a
      range of them. */
  keyLabel?: string;
}

const isMac = typeof navigator !== 'undefined' && navigator.platform.toLowerCase().includes('mac');

/** Only the keys matter here; the category and the description are the caller's
    business, so a command's own `hotkey` can be formatted as it stands. */
export function formatHotkey(def: Omit<HotkeyDef, 'category' | 'description'>): string {
  const parts: string[] = [];
  if (isMac) {
    if (def.ctrl || def.meta) parts.push('⌘');
  } else {
    if (def.ctrl) parts.push('Ctrl');
    if (def.meta) parts.push('⌘');
  }
  if (def.alt) parts.push(isMac ? '⌥' : 'Alt');
  if (def.shift) parts.push(isMac ? '⇧' : 'Shift');
  const keyMap: Record<string, string> = {
    'Enter': 'Enter',
    'Tab': 'Tab',
    'Escape': 'Esc',
    'ArrowUp': '↑',
    'ArrowDown': '↓',
    'ArrowLeft': '←',
    'ArrowRight': '→',
    'Backspace': '⌫',
    'Delete': 'Del',
  };
  const displayKey = def.keyLabel || keyMap[def.key] || def.key.toUpperCase();
  parts.push(displayKey);
  return parts.join(isMac ? '' : '+');
}

/** The physical key a shortcut means, when it means one.
 *
 *  `e.key` is what the layout produced: `⌘⇧]` arrives as `}`, `⌥L` on macOS as
 *  `¬`, and every letter shortcut arrives as Cyrillic on a Russian layout — so
 *  matching on it silently killed most of the table for anyone not typing US
 *  ASCII. The code is the key that was pressed. */
const PUNCTUATION_CODES: Record<string, string> = {
  '[': 'BracketLeft',
  ']': 'BracketRight',
  '\\': 'Backslash',
  ';': 'Semicolon',
  "'": 'Quote',
  ',': 'Comma',
  '.': 'Period',
  '/': 'Slash',
  '`': 'Backquote',
  '-': 'Minus',
  '=': 'Equal',
};

export function physicalCode(key: string): string | null {
  if (/^[a-z]$/i.test(key)) return `Key${key.toUpperCase()}`;
  if (/^[0-9]$/.test(key)) return `Digit${key}`;
  return PUNCTUATION_CODES[key] ?? null;
}

/** A shortcut with a modifier survives a focused input; a bare key does not. */
export function isChord(def: Pick<HotkeyDef, 'ctrl' | 'meta' | 'alt'>): boolean {
  return !!(def.ctrl || def.meta || def.alt);
}

export function matchesHotkey(e: KeyboardEvent, def: HotkeyDef): boolean {
  if (def.key === '?') {
    const asked = e.key === '?' || (e.code === 'Slash' && e.shiftKey);
    return asked && !e.ctrlKey && !e.metaKey && !e.altKey;
  }
  const ctrlOrMeta = def.ctrl || def.meta;
  if (ctrlOrMeta && !e.ctrlKey && !e.metaKey) return false;
  if (!ctrlOrMeta && (e.ctrlKey || e.metaKey)) return false;
  if (def.shift && !e.shiftKey) return false;
  if (!def.shift && e.shiftKey) return false;
  if (def.alt && !e.altKey) return false;
  if (!def.alt && e.altKey) return false;
  const code = physicalCode(def.key);
  return code ? e.code === code : e.key === def.key;
}

export function matchesDigitShortcut(e: KeyboardEvent): string | null {
  if (!e.altKey || e.ctrlKey || e.metaKey) return null;
  const match = e.code.match(/^Digit(\d)$/);
  if (match) {
    const n = parseInt(match[1], 10);
    if (n >= 1 && n <= 9) return match[1];
  }
  return null;
}

export function isInputFocused(): boolean {
  const el = document.activeElement;
  if (!el) return false;
  const tag = el.tagName.toLowerCase();
  if (tag === 'input' || tag === 'textarea') return true;
  if (el.getAttribute('contenteditable') === 'true') return true;
  if (el.closest('.monaco-editor')) return true;
  return false;
}

/** Whether a dialog is holding the screen.
 *
 *  A modal is a question, and the workbench's shortcuts are answers to a
 *  different one: with the palette open, ⌘⇧J opened the tools drawer behind it
 *  and ⌘W closed the tab underneath. The keys stand down until the dialog is
 *  answered — its own handlers still run, because they are inside it. */
export function modalOpen(root: Document | { querySelector: (s: string) => unknown } = document): boolean {
  return root.querySelector('dialog[open]') !== null;
}

/** Double-shift, the way an IDE opens its search.
 *
 *  Two taps of either Shift key, quickly, with nothing between them: a chord
 *  that never collides with typing, because a Shift press that produced a
 *  character is a Shift press with a key between it and the next. */
const DOUBLE_TAP_MS = 400;

export function doubleShift(gapMs: number | null): boolean {
  return gapMs !== null && gapMs > 0 && gapMs <= DOUBLE_TAP_MS;
}

/** The state a double-shift watcher keeps: when Shift last came up alone. */
export interface ShiftTap {
  lastUpAt: number | null;
}

/** What a keydown does to that state. Any key other than Shift clears it —
 *  ⇧A is typing, not a chord. */
export function noteKeyDown(state: ShiftTap, key: string): ShiftTap {
  return key === 'Shift' ? state : { lastUpAt: null };
}

/** What a keyup does. Returns the new state and whether this was the second
 *  tap — the caller opens the palette on `fired`. */
export function noteKeyUp(state: ShiftTap, key: string, now: number): { state: ShiftTap; fired: boolean } {
  if (key !== 'Shift') return { state, fired: false };
  const fired = doubleShift(state.lastUpAt === null ? null : now - state.lastUpAt);
  return { state: { lastUpAt: fired ? null : now }, fired };
}
