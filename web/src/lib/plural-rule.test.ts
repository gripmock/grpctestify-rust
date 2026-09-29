import { describe, expect, it } from 'vitest';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';

const ROOTS = ['src', 'luvo'];
const BY_HAND = /[!=]==\s*1\s*\?\s*(''|'s')\s*:\s*('s'|'')/;

function sources(dir: string): string[] {
  return readdirSync(dir).flatMap(name => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return sources(path);
    if (!/\.tsx?$/.test(name) || name.includes('.test.')) return [];
    return [path];
  });
}

describe('the plural rule', () => {
  it('is written once, not at every call site', () => {
    const web = join(import.meta.dirname, '..', '..');
    const offenders = ROOTS.flatMap(root => sources(join(web, root)))
      .filter(path => !path.endsWith(join('luvo', 'data', 'plural.ts')))
      .filter(path => BY_HAND.test(readFileSync(path, 'utf8')))
      .map(path => path.slice(web.length + 1));

    expect(offenders).toEqual([]);
  });
});
