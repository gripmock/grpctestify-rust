import { describe, expect, it } from 'vitest';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';

const ROOTS = ['src', 'luvo'];
const PLACEHOLDER = /placeholder="([^"]*)"/g;
const READS_AS_A_VALUE = /^[./]|\(\?</;

function sources(dir: string): string[] {
  return readdirSync(dir).flatMap(name => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return sources(path);
    if (!/\.tsx?$/.test(name) || name.includes('.test.')) return [];
    return [path];
  });
}

describe('the placeholders', () => {
  it('prompt for a value instead of showing one', () => {
    const web = join(import.meta.dirname, '..', '..');
    const offenders: string[] = [];

    for (const root of ROOTS) {
      for (const path of sources(join(web, root))) {
        const text = readFileSync(path, 'utf8');
        for (const [, placeholder] of text.matchAll(PLACEHOLDER)) {
          if (placeholder.includes('·')) continue;
          if (READS_AS_A_VALUE.test(placeholder)) {
            offenders.push(`${path.slice(web.length + 1)}: ${placeholder}`);
          }
        }
      }
    }

    expect(offenders).toEqual([]);
  });
});
