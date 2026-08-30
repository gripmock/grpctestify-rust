import { describe, it, expect } from 'vitest';
import { formatHotkey, matchesDigitShortcut } from 'luvo/input/hotkeys';
import { DIGIT_SHORTCUTS, LOCAL_KEYS } from './hotkeys';

describe('the tab-number keys', () => {
  it('take one row in the help, not nine', () => {
    expect(DIGIT_SHORTCUTS).toHaveLength(1);
    expect(formatHotkey(DIGIT_SHORTCUTS[0])).toContain('1 … 9');
  });

  it('still each select their tab', () => {
    for (let n = 1; n <= 9; n++) {
      const e = { altKey: true, ctrlKey: false, metaKey: false, code: `Digit${n}` } as KeyboardEvent;
      expect(matchesDigitShortcut(e)).toBe(String(n));
    }
  });

  it('ignores a digit with the wrong modifiers', () => {
    const e = { altKey: true, ctrlKey: true, metaKey: false, code: 'Digit1' } as KeyboardEvent;
    expect(matchesDigitShortcut(e)).toBeNull();
  });
});

describe('the keys inside a control', () => {
  it('cover the strips and the segmented choices, which are keyboard controls too', () => {
    const said = LOCAL_KEYS.map(k => `${k.where} ${k.keys}`).join('\n');
    expect(said).toMatch(/section tabs[\s\S]*←/);
    expect(said).toMatch(/segmented choice[\s\S]*←/);
  });
});
