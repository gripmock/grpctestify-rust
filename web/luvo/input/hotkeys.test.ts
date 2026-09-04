import { describe, it, expect } from 'vitest';
import type { HotkeyDef, ShiftTap } from './hotkeys';
import { isChord, matchesHotkey, modalOpen, noteKeyDown, noteKeyUp } from './hotkeys';

describe('matchesHotkey', () => {
  const press = (over: Partial<KeyboardEvent>) => ({
    key: '', code: '', ctrlKey: false, metaKey: false, shiftKey: false, altKey: false, ...over,
  }) as KeyboardEvent;
  const def = (over: Partial<HotkeyDef>): HotkeyDef =>
    ({ key: 'a', category: 'general', description: '', ...over });

  it('reads the key that was pressed, not the character it produced', () => {
    // ⌘⇧] arrives as `}` on a US layout, and as `ъ` on a Russian one.
    const next = def({ key: ']', ctrl: true, shift: true });
    expect(matchesHotkey(press({ key: '}', code: 'BracketRight', metaKey: true, shiftKey: true }), next)).toBe(true);
    expect(matchesHotkey(press({ key: 'ъ', code: 'BracketRight', ctrlKey: true, shiftKey: true }), next)).toBe(true);
  });

  it('matches a letter through the layout', () => {
    const save = def({ key: 's', ctrl: true });
    expect(matchesHotkey(press({ key: 'ы', code: 'KeyS', metaKey: true }), save)).toBe(true);
    expect(matchesHotkey(press({ key: 's', code: 'KeyS' }), save)).toBe(false);
  });

  it('takes ⌥L as the key under the finger, not as ¬', () => {
    const layout = def({ key: 'l', ctrl: true, alt: true });
    expect(matchesHotkey(press({ key: '¬', code: 'KeyL', metaKey: true, altKey: true }), layout)).toBe(true);
  });

  it('accepts ? from the key that carries it', () => {
    const help = def({ key: '?' });
    expect(matchesHotkey(press({ key: '?', code: 'Slash', shiftKey: true }), help)).toBe(true);
    expect(matchesHotkey(press({ key: '/', code: 'Slash', shiftKey: true }), help)).toBe(true);
    expect(matchesHotkey(press({ key: '?', code: 'Slash', shiftKey: true, metaKey: true }), help)).toBe(false);
  });

  it('knows which shortcuts survive a focused editor', () => {
    expect(isChord({ ctrl: true })).toBe(true);
    expect(isChord({ alt: true })).toBe(true);
    expect(isChord({})).toBe(false);
  });
});

describe('double-shift', () => {
  const start: ShiftTap = { lastUpAt: null };

  it('fires on two taps in quick succession', () => {
    const first = noteKeyUp(start, 'Shift', 1000);
    expect(first.fired).toBe(false);
    expect(noteKeyUp(first.state, 'Shift', 1200).fired).toBe(true);
  });

  it('does not fire when the taps are far apart', () => {
    const first = noteKeyUp(start, 'Shift', 1000);
    expect(noteKeyUp(first.state, 'Shift', 2000).fired).toBe(false);
  });

  /* ⇧A is typing a capital, not a chord: a key between the two Shifts ends it. */
  it('is cancelled by anything typed between the taps', () => {
    const first = noteKeyUp(start, 'Shift', 1000);
    const typed = noteKeyDown(first.state, 'a');
    expect(noteKeyUp(typed, 'Shift', 1100).fired).toBe(false);
  });

  it('ignores every other key', () => {
    expect(noteKeyUp(start, 'Control', 1000).fired).toBe(false);
    expect(noteKeyDown(start, 'Shift')).toBe(start);
  });

  it('starts over after it fires, so a third tap is not a fourth', () => {
    const first = noteKeyUp(start, 'Shift', 1000);
    const second = noteKeyUp(first.state, 'Shift', 1200);
    expect(second.state.lastUpAt).toBeNull();
    expect(noteKeyUp(second.state, 'Shift', 1300).fired).toBe(false);
  });
});

describe('modalOpen', () => {
  const root = (found: unknown) => ({ querySelector: () => found });

  it('is true while a dialog holds the screen', () => {
    expect(modalOpen(root({}))).toBe(true);
  });

  it('is false with nothing open', () => {
    expect(modalOpen(root(null))).toBe(false);
  });
});
