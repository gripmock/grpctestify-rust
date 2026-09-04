import type { HotkeyDef } from 'luvo/input/hotkeys';
import { formatHotkey } from 'luvo/input/hotkeys';
import { CATEGORY_LABELS, CATEGORY_ORDER, DIGIT_SHORTCUTS, GESTURE_SHORTCUTS } from './hotkeys';
import { hotkeyCommands } from './commands';

export interface KeyRow {
  keys: string;
  what: string;
  group: string;
  haystack: string;
}

export function keyWords(def: Pick<HotkeyDef, 'key' | 'ctrl' | 'shift' | 'alt' | 'meta' | 'keyLabel'>): string {
  const words: string[] = [];
  if (def.ctrl || def.meta) words.push('cmd', 'command', 'ctrl', 'control', 'meta');
  if (def.shift) words.push('shift');
  if (def.alt) words.push('alt', 'option');
  const named: Record<string, string> = {
    Enter: 'enter return',
    Escape: 'esc escape',
    Tab: 'tab',
    ArrowUp: 'up arrow',
    ArrowDown: 'down arrow',
    ArrowLeft: 'left arrow',
    ArrowRight: 'right arrow',
    Backspace: 'backspace',
    Delete: 'delete',
  };
  words.push(named[def.key] ?? def.key.toLowerCase());
  if (def.keyLabel) words.push(def.keyLabel.toLowerCase());
  return words.join(' ');
}

function row(keys: string, what: string, group: string, words = ''): KeyRow {
  return { keys, what, group, haystack: `${keys} ${words} ${what} ${group}`.toLowerCase() };
}

export function shortcutRows(): KeyRow[] {
  const rows: KeyRow[] = GESTURE_SHORTCUTS.map(g => row(g.keys, g.description, 'Gestures'));
  for (const cat of CATEGORY_ORDER) {
    const group = CATEGORY_LABELS[cat];
    const defs: (Pick<HotkeyDef, 'key' | 'ctrl' | 'shift' | 'alt' | 'meta' | 'keyLabel'> & { description: string })[] = [
      ...hotkeyCommands().filter(c => c.category === cat).map(c => ({ ...c.hotkey, description: c.title })),
      ...DIGIT_SHORTCUTS.filter(d => d.category === cat),
    ];
    for (const def of defs) rows.push(row(formatHotkey(def), def.description, group, keyWords(def)));
  }
  return rows;
}

export function filterRows(rows: KeyRow[], search: string): KeyRow[] {
  const words = search.trim().toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return rows;
  const matches = (haystack: string, word: string) =>
    word.length === 1
      ? new RegExp(`(^| )${word.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}( |$)`).test(haystack)
      : haystack.includes(word);
  return rows.filter(r => words.every(w => matches(r.haystack, w)));
}

export function groupRows(rows: KeyRow[]): { group: string; rows: KeyRow[] }[] {
  const groups: { group: string; rows: KeyRow[] }[] = [];
  for (const r of rows) {
    const last = groups[groups.length - 1];
    if (last && last.group === r.group) last.rows.push(r);
    else groups.push({ group: r.group, rows: [r] });
  }
  return groups;
}
