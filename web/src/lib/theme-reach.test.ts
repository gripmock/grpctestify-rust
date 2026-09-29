import { describe, it, expect } from 'vitest';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { FLOORS, ratioOf, resolveTheme, parseTokenBlocks } from 'luvo/theme/contrast';
import { allThemes } from 'luvo/theme/themes';

const SRC = join(import.meta.dirname, '..');
const TOKENS = readFileSync(join(SRC, '..', 'luvo', 'tokens.css'), 'utf8');
const LUVO = join(SRC, '..', 'luvo');
const APP = [
  join(SRC, 'app.css'),
  join(LUVO, 'base.css'),
  join(LUVO, 'controls.css'),
].map(f => readFileSync(f, 'utf8')).join('\n');

const UNMIGRATED = new Set<string>([]);

const PARSES_COLOUR = new Set<string>(['lib/contrast.ts']);

function walk(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) walk(full, out);
    else if (/\.tsx?$/.test(entry) && !/\.test\.tsx?$/.test(entry)) out.push(full);
  }
  return out;
}

const COLOUR_LITERAL = /#[0-9a-fA-F]{3,8}\b|rgba?\([^)]*\)/g;

describe('colour literals', () => {
  const files = walk(SRC).map(f => [relative(SRC, f), readFileSync(f, 'utf8')] as const);

  it('no component names a colour outside tokens.css', () => {
    const offenders = files
      .filter(([path]) => !UNMIGRATED.has(path) && !PARSES_COLOUR.has(path))
      .map(([path, body]) => [path, body.match(COLOUR_LITERAL) ?? []] as const)
      .filter(([, hits]) => hits.length > 0)
      .map(([path, hits]) => `${path}: ${hits.slice(0, 3).join(', ')}`);

    expect(offenders).toEqual([]);
  });

  it('the unmigrated list has no file that is already clean', () => {
    const done = files
      .filter(([path]) => UNMIGRATED.has(path))
      .filter(([, body]) => (body.match(COLOUR_LITERAL) ?? []).length === 0)
      .map(([path]) => path);

    expect(done, 'remove these from UNMIGRATED — they are already token-only').toEqual([]);
  });

  it('the unmigrated list names files that exist', () => {
    const known = new Set(files.map(([path]) => path));
    expect([...UNMIGRATED].filter(p => !known.has(p))).toEqual([]);
  });
});

const LEGACY_TOKENS = [
  '--bg-primary', '--bg-secondary', '--bg-tertiary', '--border',
  '--text-primary', '--text-secondary', '--text-muted',
  '--success', '--error', '--warning', '--accent-hover',
];

describe('the old vocabulary is gone', () => {
  const files = walk(SRC).map(f => [relative(SRC, f), readFileSync(f, 'utf8')] as const);

  it('no component reads a legacy token', () => {
    const offenders = files.flatMap(([path, body]) =>
      LEGACY_TOKENS.filter(t => body.includes(`var(${t})`)).map(t => `${path}: ${t}`));
    expect(offenders).toEqual([]);
  });

  it('tokens.css declares none of them', () => {
    const declared = new Set([...TOKENS.matchAll(/(--[\w-]+)\s*:/g)].map(m => m[1]));
    expect(LEGACY_TOKENS.filter(t => declared.has(t))).toEqual([]);
  });
});

describe('app.css', () => {
  it('names no colour of its own', () => {
    const hits = (APP.match(COLOUR_LITERAL) ?? []).filter((h: string) => !h.startsWith('rgb(0 0 0'));
    expect(hits).toEqual([]);
  });

  it('every corner comes from a radius token', () => {
    const radii = [...APP.matchAll(/border-radius:\s*([^;]+);/g)].map(m => m[1].trim());
    const allowed = (r: string) =>
      r.includes('var(--radius') || ['0', '50%', '1px'].includes(r) || r.startsWith('max(0px');
    expect(radii.filter(r => !allowed(r))).toEqual([]);
  });

  it('every duration is scaled, so reduced-motion has one lever', () => {
    const durations = [...APP.matchAll(/transition:[^;]+;/g)].map(m => m[0]);
    const unscaled = durations.filter(d => /\d+m?s/.test(d) && !d.includes('--motion-scale'));
    expect(unscaled).toEqual([]);
  });

  it('uses no Tailwind palette utility', () => {
    const palette = /\b(?:bg|text|border|ring|fill|stroke)-(?:slate|gray|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose)-\d{2,3}\b/g;
    expect(APP.match(palette) ?? []).toEqual([]);
  });

  it('reads no token that tokens.css does not declare', () => {
    const declared = new Set([...TOKENS.matchAll(/(--[\w-]+)\s*:/g)].map(m => m[1]));
    const perInstance = new Set(['--tag', '--toast-ink', '--kind', '--progress', '--kv-cols', '--depth', '--left', '--verb-ch']);
    const used = new Set([...APP.matchAll(/var\((--[\w-]+)/g)].map(m => m[1]));
    expect([...used].filter(t => !declared.has(t) && !perInstance.has(t))).toEqual([]);
  });
});

const THEMES = allThemes();
const BASE = 'terminal-light';
const OVERRIDES = THEMES.filter(t => t.id !== BASE);

describe('tokens.css', () => {
  it('has one block per theme the registry names, and nothing else', () => {
    expect(new Set(Object.keys(parseTokenBlocks(TOKENS))))
      .toEqual(new Set([':root', ...OVERRIDES.map(t => `[data-theme="${t.id}"]`)]));
  });

  it('is written in rgb(), so the grader needs no browser', () => {
    expect(TOKENS.match(/#[0-9a-fA-F]{3,8}\b/g) ?? []).toEqual([]);
  });

  it.each(OVERRIDES)('$id only overrides — it introduces no new token', theme => {
    const blocks = parseTokenBlocks(TOKENS);
    const base = new Set(Object.keys(blocks[':root']));
    expect(Object.keys(blocks[`[data-theme="${theme.id}"]`]).filter(t => !base.has(t))).toEqual([]);
  });

  it.each(OVERRIDES)('$id repaints every colour the other themes do', theme => {
    const blocks = parseTokenBlocks(TOKENS);
    const expected = new Set(OVERRIDES.flatMap(t => Object.keys(blocks[`[data-theme="${t.id}"]`])));
    const own = new Set(Object.keys(blocks[`[data-theme="${theme.id}"]`]));
    expect([...expected].filter(t => !own.has(t))).toEqual([]);
  });
});

describe.each(THEMES)('contrast — $id', theme => {
  const tokens = resolveTheme(TOKENS, theme.id);

  it.each(FLOORS)('%s on %s clears %f', (fg, bg, min) => {
    const ratio = ratioOf(tokens, fg, bg);
    expect(ratio, `${fg} on ${bg} is unreadable or unparseable`).not.toBeNull();
    expect(ratio!).toBeGreaterThanOrEqual(min);
  });
});

describe('the splitter drives a pane', () => {
  it('gives the request pane the height the handle holds', () => {
    const rule = APP.match(/\.workspace\.is-rows:not\(\.is-form\) > \.request-pane \{[^}]*\}/);
    expect(rule, 'the rows request pane must read --editor-h').not.toBeNull();
    expect(rule![0]).toContain('height: var(--editor-h)');
  });

  it('lets the request take the room while nothing has come back', () => {
    const rule = APP.match(/\.workspace\.is-rows\.is-idle:not\(\.is-form\) > \.request-pane \{[^}]*\}/);
    expect(rule, 'an idle workbench has no split to hold').not.toBeNull();
    expect(rule![0]).toContain('height: auto');
  });

  it('keeps a gap around the handle, so it is not a pane border', () => {
    const rule = APP.match(/\n\.workspace \{[^}]*\}/);
    expect(rule![0]).toContain('gap: var(--gap-2)');
  });
});

describe('geometry lives in the sheet', () => {
  const files = walk(SRC).map(f => [relative(SRC, f), readFileSync(f, 'utf8')] as const);
  const LENGTH = /style=\{\{[^}]*?['"`]\s*[\d.]+(?:rem|px|em)\b/g;

  it('no component writes a length in an inline style', () => {
    const offenders = files
      .map(([path, body]) => [path, body.match(LENGTH) ?? []] as const)
      .filter(([, hits]) => hits.length > 0)
      .map(([path, hits]) => `${path}: ${hits[0]}`);

    expect(offenders).toEqual([]);
  });
});

describe('no selector is declared twice in a layer', () => {
  const SHEETS = {
    'app.css': readFileSync(join(SRC, 'app.css'), 'utf8'),
    'controls.css': readFileSync(join(SRC, '..', 'luvo', 'controls.css'), 'utf8'),
    'base.css': readFileSync(join(SRC, '..', 'luvo', 'base.css'), 'utf8'),
  };

  function topLevelSelectors(css: string): string[] {
    const withoutComments = css.replace(/\/\*[\s\S]*?\*\//g, '');
    const out: string[] = [];
    let depth = 0;
    let head = '';
    for (const ch of withoutComments) {
      if (ch === '{') {
        depth += 1;
        if (depth === 1) {
          const selector = head.replace(/\s+/g, ' ').trim();
          if (selector !== '' && !selector.startsWith('@')) out.push(selector);
          if (selector.startsWith('@')) depth = -1000;
        }
        head = '';
      } else if (ch === '}') {
        depth = depth <= -900 ? (depth === -1000 ? 0 : depth + 1) : depth - 1;
        head = '';
      } else if (depth === 0) {
        head += ch;
      }
    }
    return out;
  }

  it.each(Object.keys(SHEETS))('%s declares each selector once', name => {
    const seen = new Map<string, number>();
    for (const selector of topLevelSelectors(SHEETS[name as keyof typeof SHEETS])) {
      seen.set(selector, (seen.get(selector) ?? 0) + 1);
    }
    expect([...seen].filter(([, n]) => n > 1).map(([s]) => s)).toEqual([]);
  });
});

describe('control metrics', () => {
  const CONTROLS = ['.btn', '.badge', '.chip', '.seg > button', '.field', '.tab'];

  it('controls own their line-height, so Tailwind preflight cannot inflate them', () => {
    const block = APP.match(/\.btn, \.badge[^{]*\{[^}]*\}/);
    expect(block, 'the control line-height block must exist').not.toBeNull();
    expect(block![0]).toContain('line-height: var(--line-control)');
    for (const c of CONTROLS) {
      const selector = c.replace('.', '\\.').replace(' > ', ', ');
      expect(block![0].includes(c) || APP.includes(`${c} {`), `${selector} must be covered`).toBe(true);
    }
  });

  it('control heights come from the height scale — only hairlines and dots may be raw px', () => {
    const heights = [...APP.matchAll(/(?:^|\n)\s*(?:min-)?height:\s*([^;]+);/g)].map(m => m[1].trim());
    const allowed = (h: string) =>
      h.includes('var(--h-') || h.includes('rem') || h.includes('%') || h.includes('vh') ||
      h.includes('calc(') || h === 'auto' || h === '0' || h.includes('em') ||
      /^[1-9]px$/.test(h) || h === 'fit-content' || h.includes('var(--output-edge)') ||
      h === 'var(--editor-h)';
    expect(heights.filter(h => !allowed(h))).toEqual([]);
  });

  it('spaces on the scale — a raw rem gap or padding is how ten steps came back', () => {
    const props = /(?:^|\n)\s*(?:gap|row-gap|column-gap|padding|padding-inline|padding-block|padding-top|padding-right|padding-bottom|padding-left|margin-top|margin-bottom):\s*([^;]+);/g;
    const offenders = [...APP.matchAll(props)]
      .map(m => m[1].trim())
      .filter(v => !v.includes('var(') && !v.includes('calc(') && /rem/.test(v));
    expect(offenders).toEqual([]);
  });

  it('declares the metric tokens the controls read', () => {
    for (const token of ['--line-control', '--line-text', '--h-badge', '--h-sm', '--h-seg', '--h-control']) {
      expect(TOKENS, `${token} must be declared`).toContain(`${token}:`);
    }
  });
});

const DARK_PALETTE: Record<string, string> = {
  '--surface': 'rgb(14, 16, 18)',
  '--surface-sunken': 'rgb(8, 10, 11)',
  '--surface-raised': 'rgb(22, 25, 27)',
  '--surface-hover': 'rgb(30, 34, 36)',
  '--row-alt': 'rgb(17, 20, 21)',
  '--line': 'rgb(39, 44, 46)',
  '--line-strong': 'rgb(62, 69, 71)',
  '--ink': 'rgb(226, 232, 228)',
  '--ink-muted': 'rgb(139, 151, 145)',
  '--accent': 'rgb(226, 165, 88)',
  '--accent-fill': 'rgb(226, 165, 88)',
  '--accent-ink': 'rgb(14, 16, 18)',
  '--ok': 'rgb(116, 202, 156)',
  '--fail': 'rgb(226, 108, 105)',
  '--warn': 'rgb(216, 184, 106)',
  '--kind-simple': 'rgb(112, 190, 184)',
  '--kind-down': 'rgb(120, 170, 214)',
  '--kind-up': 'rgb(172, 148, 210)',
  '--kind-duplex': 'rgb(210, 156, 100)',
};

describe('the dark palette stays what the design settled on', () => {
  const dark = resolveTheme(TOKENS, 'terminal-dark');

  it.each(Object.entries(DARK_PALETTE))('%s is %s', (token, value) => {
    expect(dark[token]).toBe(value);
  });
});

const EDGES: ReadonlyArray<readonly [string, string, number]> = [
  ['--line', '--surface', 1.4],
  ['--line', '--surface-raised', 1.5],
  ['--line-strong', '--surface-raised', 2.2],
];

const DARK_EDGES: ReadonlyArray<readonly [string, string, number]> = [
  ['--line', '--surface', 1.3],
  ['--line', '--surface-raised', 1.2],
  ['--line-strong', '--surface-raised', 1.7],
];

describe.each(THEMES)('edges — $id', theme => {
  const tokens = resolveTheme(TOKENS, theme.id);

  it.each(theme.mode === 'light' ? EDGES : DARK_EDGES)('%s on %s clears %f', (fg, bg, min) => {
    const ratio = ratioOf(tokens, fg, bg);
    expect(ratio, `${fg} on ${bg}`).not.toBeNull();
    expect(ratio!).toBeGreaterThanOrEqual(min);
  });
});
