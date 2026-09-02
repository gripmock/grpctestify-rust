import { describe, it, expect } from 'vitest';
import { readFileSync, readdirSync } from 'node:fs';
import { join, relative } from 'node:path';

const SRC = import.meta.dirname;
const CSS = [
  join(SRC, '..', 'app.css'),
  join(SRC, '..', '..', 'luvo', 'controls.css'),
  join(SRC, '..', '..', 'luvo', 'base.css'),
].map(f => readFileSync(f, 'utf8')).join('\n');

interface Family {
  base: string;
  from: string;
  styled: string[];
  plain?: string[];
}

const FAMILIES: Family[] = [
  {
    base: '.rail-dot', from: "stepMarks() in lib/jobs.ts",
    styled: ['is-pass', 'is-fail', 'is-skip'], plain: ['is-none'],
  },
  {
    base: '.problem', from: "severityLabel() in lib/problems.ts",
    styled: ['is-error', 'is-warning', 'is-info'],
  },
  {
    base: '.history-payload', from: "callSummary() in lib/history-group.ts",
    styled: ['is-response', 'is-error'], plain: ['is-request'],
  },
  {
    base: '.extract-value', from: 'ExtractValue in lib/extract-preview.ts',
    styled: ['is-value', 'is-many', 'is-none', 'is-null', 'is-error'],
  },
  {
    base: '.kvrow', from: 'checkMetadataKey() in lib/metadata.ts',
    styled: ['is-bad', 'is-note'],
  },
  {
    base: '.summary .count', from: 'RunBar’s pass/fail/skip counts',
    styled: ['is-ok', 'is-fail', 'is-skip'],
  },
  {
    base: '', from: 'familyOf() in lib/tree.ts',
    styled: ['is-gctf', 'is-httf', 'is-apif', 'is-unknown'],
  },
  {
    base: '.toast', from: "ToastContext's kind",
    styled: ['is-ok', 'is-fail', 'is-info'],
  },
];

describe('every state a component can write has a rule', () => {
  it.each(FAMILIES.flatMap(f => f.styled.map(state => [f.base, state, f.from] as const)))(
    '%s.%s — %s',
    (base, state, _from) => {
      const selector = `${base}.${state}`.replace(/([.*+?^${}()|[\]\\])/g, '\\$1');
      expect(new RegExp(selector).test(CSS), `${base}${state} has no rule`).toBe(true);
    },
  );

  it('and the plain states are named rather than forgotten', () => {
    const plain = FAMILIES.flatMap(f => (f.plain ?? []).map(state => `${f.base}.${state}`));
    expect(plain).toEqual(['.rail-dot.is-none', '.history-payload.is-request']);
  });
});

const COMPONENT_ROOTS = [
  join(SRC, '..'),
  join(SRC, '..', '..', 'luvo'),
];

function tsxFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap(entry => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return tsxFiles(path);
    return entry.name.endsWith('.tsx') && !entry.name.endsWith('.test.tsx') ? [path] : [];
  });
}

interface Scan {
  tokens: string[];
  end: number;
}

function compared(source: string, from: number, to: number): boolean {
  const before = source.slice(Math.max(0, from - 6), from);
  return /[=!]==?\s*$/.test(before)
    || /\(\s*$/.test(before)
    || /^\s*[=!]==?/.test(source.slice(to, to + 6));
}

const CLASS_SHAPED = /^[A-Za-z][A-Za-z0-9_-]*$/;

function scanString(source: string, from: number, quote: string): { text: string; end: number } {
  let text = '';
  let i = from;
  while (i < source.length) {
    const c = source[i];
    if (c === '\\') { i += 2; continue; }
    if (c === quote) return { text, end: i + 1 };
    text += c;
    i += 1;
  }
  return { text, end: i };
}

function scanTemplate(source: string, from: number): Scan {
  const tokens: string[] = [];
  let chunk = '';
  let i = from;
  while (i < source.length) {
    const c = source[i];
    if (c === '\\') { i += 2; continue; }
    if (c === '`') { tokens.push(...chunk.split(/\s+/)); return { tokens, end: i + 1 }; }
    if (c === '$' && source[i + 1] === '{') {
      tokens.push(...chunk.split(/\s+/));
      chunk = '';
      const inner = scanExpression(source, i + 2);
      tokens.push(...inner.tokens);
      i = inner.end;
      continue;
    }
    chunk += c;
    i += 1;
  }
  tokens.push(...chunk.split(/\s+/));
  return { tokens, end: i };
}

function scanExpression(source: string, from: number): Scan {
  const tokens: string[] = [];
  let depth = 0;
  let i = from;
  while (i < source.length) {
    const c = source[i];
    if (c === '{') { depth += 1; i += 1; continue; }
    if (c === '}') {
      if (depth === 0) return { tokens, end: i + 1 };
      depth -= 1;
      i += 1;
      continue;
    }
    if (c === '\'' || c === '"') {
      const said = scanString(source, i + 1, c);
      if (!compared(source, i, said.end)) tokens.push(...said.text.split(/\s+/));
      i = said.end;
      continue;
    }
    if (c === '`') {
      const said = scanTemplate(source, i + 1);
      tokens.push(...said.tokens);
      i = said.end;
      continue;
    }
    i += 1;
  }
  return { tokens, end: i };
}

export function classTokens(source: string): string[] {
  const found: string[] = [];
  for (const m of source.matchAll(/className="([^"]*)"/g)) found.push(...m[1].split(/\s+/));
  for (const m of source.matchAll(/className=\{/g)) {
    found.push(...scanExpression(source, m.index + 'className={'.length).tokens);
  }
  return found.filter(t => CLASS_SHAPED.test(t) && !t.endsWith('-'));
}

export function hasRule(css: string, token: string): boolean {
  const escaped = token.replace(/([.*+?^${}()|[\]\\])/g, '\\$1');
  return new RegExp(`\\.${escaped}(?![\\w-])`).test(css);
}

const UNSTYLED: Record<string, string> = {
  'diff-text': 'names the text span of a diff line for tests and future rules',
  'env-repeated': 'names the warning note about a repeated variable',
  'env-target-scheme': 'names the note under the environment address',
  'expect': 'names the EXPECT editor root',
  'extract-unbound': 'names the list of extracts nothing answered',
  'env-target-graded': 'names the graded note under an environment address — the note carries the style',
  'history-reread': 'names the button that reads the project’s record again, for HistoryPanel.test',
  'history-search-mark': 'names the search icon in the history filter',
  'http-split': 'names the button that splits a typed url into address and path',
  'is-series': 'the legend swatch in its default state — only is-target restyles it',
  'ok': 'JqGolf’s score line — outside this scan’s owner; a candidate for is-ok',
  'peek-head': 'names the head row of the history card',
  'plan-section': 'names a plan row that carries a section name',
  'rail-chips': 'names the active filter chips of the rail',
  'run-cases': 'names the per-case note of a run summary',
  'save-family': 'names the family choice in the save dialog — SaveDialog.test clicks it',
  'tag-menu': 'names the tag filter popover',
  'thresholds': 'names the thresholds panel body of the bench editor',
};

describe('every class a component writes has a rule', () => {
  const seen = new Map<string, Set<string>>();
  for (const root of COMPONENT_ROOTS) {
    for (const file of tsxFiles(root)) {
      const source = readFileSync(file, 'utf8');
      for (const token of classTokens(source)) {
        const at = seen.get(token) ?? new Set<string>();
        at.add(relative(join(SRC, '..', '..'), file));
        seen.set(token, at);
      }
    }
  }

  it('or is named as a hook that carries no style', () => {
    const missing = [...seen.entries()]
      .filter(([token]) => !hasRule(CSS, token) && !(token in UNSTYLED))
      .map(([token, files]) => `${token} — ${[...files].join(', ')}`)
      .sort();
    expect(missing).toEqual([]);
  });

  it('and the hooks named as unstyled are still unstyled and still written', () => {
    const stale = Object.keys(UNSTYLED).filter(token => !seen.has(token) || hasRule(CSS, token));
    expect(stale).toEqual([]);
  });

  it('catches a spinner class no stylesheet knows', () => {
    expect(hasRule(CSS, 'spin')).toBe(false);
    expect(hasRule(CSS, 'animate-spin')).toBe(true);
  });

  it('reads every class named in the attribute, however it is written', () => {
    expect(classTokens('className="btn is-sm"')).toEqual(['btn', 'is-sm']);
    expect(classTokens('className={`chip is-${tone}`}')).toEqual(['chip']);
    expect(classTokens("className={open ? 'is-on' : 'is-off'}")).toEqual(['is-on', 'is-off']);
    expect(classTokens("className={`row${open ? ' is-on' : ''}`}")).toEqual(['row', 'is-on']);
  });

  it('cannot see a class a variable was assembled from, and says so', () => {
    expect(classTokens('const name = `is-${tone}`; <span className={name} />')).toEqual([]);
  });

  it('tells a class apart from the value the choice was made on', () => {
    expect(classTokens("className={kind === 'error' ? 'is-fail' : 'is-ok'}")).toEqual(['is-fail', 'is-ok']);
    expect(classTokens("className={`chip${'browser' === source ? ' is-on' : ''}`}")).toEqual(['chip', 'is-on']);
    expect(classTokens("className={`opt${beaten('timeout') ? ' is-overruled' : ''}`}")).toEqual(['opt', 'is-overruled']);
  });

  it('does not mistake an argument that could never be a class for one', () => {
    expect(classTokens("className={v.includes('{{') ? 'is-templated' : 'plain-row'}")).toEqual(['is-templated', 'plain-row']);
  });

  it('would not see a class handed straight to a helper, which is why none is used', () => {
    expect(classTokens("className={cx('badge', bad && 'is-fail')}")).toEqual(['is-fail']);
  });
});
