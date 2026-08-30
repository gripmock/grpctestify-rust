/** The one rule luvo has: it must not know what a `.gctf` file is.
 *
 *  A framework that imports from the app it was extracted from is not a
 *  framework, it is a folder. This is what keeps the split real: the day
 *  someone reaches back into `src/` for a type, the suite says so. */

import { describe, it, expect } from 'vitest';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';

const ROOT = import.meta.dirname;

function walk(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) walk(full, out);
    else if (/\.(ts|tsx|css)$/.test(entry)) out.push(full);
  }
  return out;
}

const files = walk(ROOT).map(f => [relative(ROOT, f), readFileSync(f, 'utf8')] as const);

describe('luvo knows nothing about the app', () => {
  it('imports nothing from src/', () => {
    const offenders = files
      .filter(([, body]) => /from\s+'[^']*(?:\.\.\/)*src\//.test(body) || /@import\s+"[^"]*src\//.test(body))
      .map(([path]) => path);
    expect(offenders).toEqual([]);
  });

  /* `.gctf`, endpoints, asserts, benches: the words of the product. A comment
     may cite the bug that motivated a primitive — the code may not. */
  it('names no part of the file format in its code', () => {
    const product = /\b(gctf|httf|ASSERTS|ENDPOINT|grpc[A-Z_]|CollectionParsed|useStore)\b/;
    const offenders = files
      .filter(([path]) => !path.endsWith('.test.ts') && path !== 'README.md')
      .map(([path, body]) => {
        const code = body
          .replace(/\/\*[\s\S]*?\*\//g, '')
          .replace(/^\s*\/\/.*$/gm, '');
        return [path, code.match(product) ?? []] as const;
      })
      .filter(([, hits]) => hits.length > 0)
      .map(([path, hits]) => `${path}: ${hits[0]}`);
    expect(offenders).toEqual([]);
  });

  it('is where the design tokens live', () => {
    expect(files.some(([path]) => path === 'tokens.css')).toBe(true);
  });
});

/* The split is by cascade order: luvo is imported first and the app's layer
   after it, so a primitive that stays behind in `app.css` is a primitive the
   app can only reach by name. These are the ones already moved — the list may
   grow, and a rule that walks back into the app fails here. */
describe('the control vocabulary lives in luvo', () => {
  const controls = readFileSync(join(ROOT, 'controls.css'), 'utf8');
  const app = readFileSync(join(ROOT, '..', 'src', 'app.css'), 'utf8');

  const OWNED = [
    '.btn', '.field', '.badge', '.chip', '.seg', '.menu', '.menu-item', '.row', '.kv', '.kvrow',
    '.modal', '.modal-head', '.modal-body', '.modal-foot', '.toast', '.tabs', '.tab',
    '.split', '.hsplit', '.panel', '.card', '.empty', '.kbd', '.note', '.dot', '.stack', '.bar',
    '.diff', '.tile', '.tiles', '.editor', '.grow', '.picker',
  ];

  it.each(OWNED)('%s is defined here', selector => {
    const rule = new RegExp(`(?:^|\n)\\${selector}[\\s,{:]`);
    expect(rule.test(controls), `${selector} must be a luvo rule`).toBe(true);
  });

  /* Two meanings under one class is how `.field-frame.is-bad` came to be
     declared twice, in two colours, with the second silently winning. */
  it('declares each selector once', () => {
    const twice = OWNED.filter(selector => {
      const bare = new RegExp(`(?:^|\n)\\${selector}\\s*\\{`, 'g');
      return (controls.match(bare) ?? []).length > 1;
    });
    expect(twice).toEqual([]);
  });

  it('and the app defines none of them again at the same weight', () => {
    const offenders = OWNED.filter(selector => {
      const bare = new RegExp(`(?:^|\n)\\${selector}\\s*\\{`);
      return bare.test(app);
    });
    expect(offenders).toEqual([]);
  });
});
