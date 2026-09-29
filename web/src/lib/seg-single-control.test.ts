import { describe, expect, it } from 'vitest';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';

const COMPONENTS = join(import.meta.dirname, '..', 'components');

function walk(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) walk(full, out);
    else if (/\.tsx$/.test(entry) && !/\.test\.tsx$/.test(entry)) out.push(full);
  }
  return out;
}

describe('the segmented controls', () => {
  it('are all the one control', () => {
    const handwritten = walk(COMPONENTS)
      .filter(f => /className=("seg\b|\{`seg\b)/.test(readFileSync(f, 'utf8')))
      .map(f => relative(COMPONENTS, f));
    expect(handwritten).toEqual([]);
  });
});
