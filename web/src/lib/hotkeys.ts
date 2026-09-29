import type { HotkeyDef } from 'luvo/input/hotkeys';

export const DIGIT_SHORTCUTS: HotkeyDef[] = [{
  key: '1',
  alt: true,
  category: 'tabs' as const,
  description: 'Select a tab by number',
  keyLabel: '1 … 9',
}];

export const GESTURE_SHORTCUTS: { keys: string; description: string }[] = [
  { keys: '⇧ ⇧', description: 'Open the palette — two taps of Shift' },
];

export const CATEGORY_LABELS: Record<string, string> = {
  execution: 'Execution',
  run: 'Runs',
  file: 'File',
  tabs: 'Tabs',
  navigation: 'Navigation',
  general: 'General',
};

export const LOCAL_KEYS: { where: string; keys: string }[] = [
  { where: 'the file tree', keys: '↑ ↓ walk · → ← open and close a folder · Enter opens the file' },
  { where: 'the tab strip', keys: '← → walk · Home End ends · Enter keeps a preview · Delete closes · ⇧F10 menu' },
  { where: 'the method list and the palette', keys: '↑ ↓ walk · PgUp PgDn by eight · Home End ends · Enter picks · Esc closes' },
  { where: 'the section tabs — request, headers, expect…', keys: '← → walk · Home End ends' },
  { where: 'a segmented choice — protocol, security, family…', keys: '← → pick · Home End ends' },
  { where: 'a chain', keys: 'the step dots take Enter and Space' },
  { where: 'history', keys: 'Space shows the call · Enter opens it in a tab · ↑ ↓ walk the list from the panel' },
  { where: 'a splitter', keys: 'arrows nudge · ⇧ moves four steps · Home End to the limits' },
  { where: 'the tools drawer', keys: 'jq · regex · schema — Esc closes it' },
];

export const CATEGORY_ORDER = ['execution', 'run', 'file', 'tabs', 'navigation', 'general'] as const;
