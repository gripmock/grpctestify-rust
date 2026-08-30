import { describe, it, expect } from 'vitest';
import { allThemes, MODES, nextMode, PALETTES, paletteOf, readChoice, resolveMode, themeId } from 'luvo/theme/themes';

describe('readChoice', () => {
  it('reads a stored palette and mode', () => {
    expect(readChoice(JSON.stringify({ palette: 'slate', mode: 'light' })))
      .toEqual({ palette: 'slate', mode: 'light' });
  });

  /* Every name this setting was ever written as, because a workbench that
     forgets the theme on upgrade is a workbench that changed colour by itself. */
  it('carries every value written before palettes and modes were told apart', () => {
    expect(readChoice('light')).toEqual({ palette: 'terminal', mode: 'light' });
    expect(readChoice('paper')).toEqual({ palette: 'terminal', mode: 'light' });
    expect(readChoice('dark')).toEqual({ palette: 'terminal', mode: 'dark' });
    expect(readChoice('terminal')).toEqual({ palette: 'terminal', mode: 'dark' });
    expect(readChoice('slate')).toEqual({ palette: 'slate', mode: 'dark' });
    expect(readChoice('contrast')).toEqual({ palette: 'slate', mode: 'dark' });
  });

  it('follows the OS for anything it does not know', () => {
    expect(readChoice(null)).toEqual({ palette: 'terminal', mode: 'system' });
    expect(readChoice('sepia')).toEqual({ palette: 'terminal', mode: 'system' });
    expect(readChoice(JSON.stringify({ palette: 'nope', mode: 'nope' })))
      .toEqual({ palette: 'terminal', mode: 'system' });
  });
});

describe('resolveMode', () => {
  it('follows the OS only while nothing was chosen', () => {
    expect(resolveMode('system', true)).toBe('dark');
    expect(resolveMode('system', false)).toBe('light');
    expect(resolveMode('light', true)).toBe('light');
    expect(resolveMode('dark', false)).toBe('dark');
  });
});

describe('the registry', () => {
  it('gives every palette both modes', () => {
    expect(allThemes().map(t => t.id)).toEqual([
      'terminal-light', 'terminal-dark', 'slate-light', 'slate-dark',
      'plum-light', 'plum-dark', 'mono-light', 'mono-dark',
    ]);
  });

  it('names every palette and every mode once', () => {
    expect(new Set(PALETTES.map(p => p.id)).size).toBe(PALETTES.length);
    expect(new Set(MODES.map(m => m.id)).size).toBe(MODES.length);
  });

  it('answers for a palette it does not have rather than crashing the app', () => {
    expect(paletteOf('nope').id).toBe('terminal');
  });

  it('writes one attribute value per palette and mode', () => {
    expect(themeId('slate', 'light')).toBe('slate-light');
  });
});

describe('nextMode', () => {
  it('steps through the three and comes back', () => {
    expect(nextMode('light')).toBe('dark');
    expect(nextMode('dark')).toBe('system');
    expect(nextMode('system')).toBe('light');
  });
});
