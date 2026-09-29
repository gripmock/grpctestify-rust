import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { toHex, themeRules, themeColors, applyEditorTheme, EDITOR_THEME } from './monaco-theme';

const tokens = {
  ink: '171716', inkMuted: '5e5d5a', surface: 'ffffff', line: 'cecdca',
  accent: 'a6540a', accentSoft: 'a6540a', lineStrong: '989693', string: '0e7474', number: '1d58a8',
};

describe('toHex', () => {
  it('reads the form tokens.css is written in', () => {
    expect(toHex('rgb(23, 23, 22)')).toBe('171716');
    expect(toHex('  rgb(255 183 77)  ')).toBe('ffb74d');
    expect(toHex('rgba(0, 0, 0, 0.5)')).toBe('000000');
  });

  it('reads the hex the minifier writes', () => {
    expect(toHex('#171716')).toBe('171716');
    expect(toHex('#E2E8E4')).toBe('e2e8e4');
    expect(toHex('#abc')).toBe('aabbcc');
    expect(toHex('#16191bff')).toBe('16191b');
  });

  it('is nothing for anything else', () => {
    expect(toHex('')).toBeNull();
    expect(toHex('var(--ink)')).toBeNull();
    expect(toHex('#12')).toBeNull();
  });
});

describe('themeRules', () => {
  it('gives a JSON key the ink and a value the string colour', () => {
    const rules = themeRules(tokens);
    expect(rules.find(r => r.token === 'string.key.json')?.foreground).toBe('171716');
    expect(rules.find(r => r.token === 'string.value.json')?.foreground).toBe('0e7474');
    expect(rules.find(r => r.token === 'comment')?.fontStyle).toBe('italic');
  });
});

describe('themeColors', () => {
  it('paints the editor on the panel it sits in', () => {
    const colors = themeColors(tokens, 'light');
    expect(colors['editor.background']).toBe('#ffffff');
    expect(colors['editor.foreground']).toBe('#171716');
    expect(colors['scrollbarSlider.background']).toBe('#98969355');
  });
});

describe('applyEditorTheme', () => {
  it('defines and selects one theme, and says so', () => {
    const calls: string[] = [];
    const monaco = {
      editor: {
        defineTheme: (name: string) => calls.push(`define:${name}`),
        setTheme: (name: string) => calls.push(`set:${name}`),
      },
    };
    const root = document.createElement('div');
    root.style.setProperty('--ink', 'rgb(23, 23, 22)');
    root.style.setProperty('--ink-muted', 'rgb(94, 93, 90)');
    root.style.setProperty('--surface-raised', 'rgb(255, 255, 255)');
    root.style.setProperty('--line', 'rgb(206, 205, 202)');
    root.style.setProperty('--accent', 'rgb(166, 84, 10)');
    root.style.setProperty('--line-strong', 'rgb(152, 150, 147)');
    root.style.setProperty('--kind-simple', 'rgb(14, 116, 116)');
    root.style.setProperty('--kind-down', 'rgb(29, 88, 168)');
    document.body.append(root);

    expect(applyEditorTheme(monaco, 'light', root)).toBe(true);
    expect(calls).toEqual([`define:${EDITOR_THEME}`, `set:${EDITOR_THEME}`]);
  });

  it('declines rather than painting half a theme', () => {
    const monaco = { editor: { defineTheme: () => {}, setTheme: () => {} } };
    const bare = document.createElement('div');
    document.body.append(bare);
    expect(applyEditorTheme(monaco, 'light', bare)).toBe(false);
  });
});

describe('what tells the editors the theme changed', () => {
  const layout = readFileSync(
    join(import.meta.dirname, '..', 'components', 'layout', 'PlayLayout.tsx'),
    'utf8',
  );

  it('watches the palette as well as the mode', () => {
    expect(layout).toMatch(/useEffect\(\(\) => \{ retheme\(themeMode\); \}, \[themeMode, palette\]\)/);
  });

  it('does not wait for a frame that a re-render can cancel', () => {
    const around = layout.slice(Math.max(0, layout.indexOf('retheme(themeMode)') - 400), layout.indexOf('retheme(themeMode)') + 200);
    expect(around).not.toContain('requestAnimationFrame');
  });
});
