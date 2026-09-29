import { describe, expect, it } from 'vitest';
import { filterRows, groupRows, keyWords, shortcutRows } from './shortcut-rows';

describe('a keycap has words', () => {
  it('knows what its modifiers are called', () => {
    const words = keyWords({ key: 'r', ctrl: true, shift: true });
    for (const said of ['cmd', 'ctrl', 'command', 'shift', 'r']) expect(words).toContain(said);
  });

  it('names the keys that are not letters', () => {
    expect(keyWords({ key: 'Enter', ctrl: true })).toContain('return');
    expect(keyWords({ key: 'ArrowUp' })).toContain('up');
  });

  it('carries the label a range of keys is written as', () => {
    expect(keyWords({ key: '1', alt: true, keyLabel: '1 … 9' })).toContain('1 … 9');
  });
});

describe('the filter', () => {
  const rows = shortcutRows();

  it('finds a shortcut by the words for its keys', () => {
    const hits = filterRows(rows, 'cmd shift r');
    expect(hits.map(r => r.what)).toEqual(['Run — the current scope']);
  });

  it('reads a single letter as the key itself', () => {
    expect(filterRows(rows, 'w').map(r => r.what)).toEqual(['Close current tab']);
  });

  it('does not care in which order the words were typed', () => {
    expect(filterRows(rows, 'close tab').map(r => r.what))
      .toEqual(filterRows(rows, 'tab close').map(r => r.what));
  });

  it('finds a group by its name', () => {
    expect(filterRows(rows, 'navigation').length).toBeGreaterThan(0);
  });

  it('keeps everything when nothing is typed', () => {
    expect(filterRows(rows, '   ')).toHaveLength(rows.length);
  });

  it('finds nothing for a word nothing has', () => {
    expect(filterRows(rows, 'zzzz')).toHaveLength(0);
  });
});

describe('the list', () => {
  it('opens with the gesture that is not a key plus modifiers', () => {
    expect(shortcutRows()[0].group).toBe('Gestures');
  });

  it('has no group without rows', () => {
    const groups = groupRows(filterRows(shortcutRows(), 'tab'));
    expect(groups.length).toBeGreaterThan(0);
    expect(groups.every(g => g.rows.length > 0)).toBe(true);
  });

  it('keeps each group together and in order', () => {
    const names = groupRows(shortcutRows()).map(g => g.group);
    expect(new Set(names).size).toBe(names.length);
  });
});
