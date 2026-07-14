export interface HotkeyDef {
  key: string;
  ctrl?: boolean;
  shift?: boolean;
  alt?: boolean;
  meta?: boolean;
  category: 'tabs' | 'navigation' | 'execution' | 'general';
  description: string;
}

const isMac = typeof navigator !== 'undefined' && navigator.platform.toLowerCase().includes('mac');

export function formatHotkey(def: HotkeyDef): string {
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
  const displayKey = keyMap[def.key] || def.key.toUpperCase();
  parts.push(displayKey);
  return parts.join(isMac ? '' : '+');
}

export function matchesHotkey(e: KeyboardEvent, def: HotkeyDef): boolean {
  if (def.key === '?') {
    return e.key === '?' && !e.ctrlKey && !e.metaKey && !e.altKey;
  }
  const ctrlOrMeta = def.ctrl || def.meta;
  if (ctrlOrMeta && !e.ctrlKey && !e.metaKey) return false;
  if (!ctrlOrMeta && (e.ctrlKey || e.metaKey)) return false;
  if (def.shift && !e.shiftKey) return false;
  if (!def.shift && e.shiftKey) return false;
  if (def.alt && !e.altKey) return false;
  if (!def.alt && e.altKey) return false;
  return e.key === def.key;
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

export const SHORTCUT_DEFINITIONS: HotkeyDef[] = [
  { key: 'Enter', ctrl: true, category: 'execution', description: 'Execute request' },
  { key: 'w', ctrl: true, category: 'tabs', description: 'Close current tab' },
  { key: 'T', ctrl: true, shift: true, category: 'tabs', description: 'New tab' },
  { key: ']', ctrl: true, shift: true, category: 'tabs', description: 'Next tab' },
  { key: '[', ctrl: true, shift: true, category: 'tabs', description: 'Previous tab' },
  { key: 'b', ctrl: true, category: 'navigation', description: 'Toggle sidebar' },
  { key: '?', category: 'general', description: 'Keyboard shortcuts help' },
];

for (let i = 1; i <= 9; i++) {
  SHORTCUT_DEFINITIONS.push({
    key: String(i),
    alt: true,
    category: 'tabs',
    description: `Select tab ${i}`,
  });
}

export const CATEGORY_LABELS: Record<string, string> = {
  tabs: 'Tabs',
  navigation: 'Navigation',
  execution: 'Execution',
  general: 'General',
};

export const CATEGORY_ORDER = ['tabs', 'navigation', 'execution', 'general'] as const;
