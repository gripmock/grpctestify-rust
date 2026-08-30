import { describe, it, expect } from 'vitest';
import { lineDiff, hasChanges } from 'luvo/data/diff';

describe('lineDiff', () => {
  it('reports no change for identical text', () => {
    const d = lineDiff('a\nb', 'a\nb');
    expect(hasChanges(d)).toBe(false);
    expect(d.map(x => x.text)).toEqual(['a', 'b']);
  });

  it('marks an inserted line as an addition only', () => {
    const d = lineDiff('a\nc', 'a\nb\nc');
    expect(d).toEqual([
      { kind: 'same', text: 'a' },
      { kind: 'add', text: 'b' },
      { kind: 'same', text: 'c' },
    ]);
  });

  it('marks a removed line as a deletion only', () => {
    const d = lineDiff('a\nb\nc', 'a\nc');
    expect(d.filter(x => x.kind === 'del').map(x => x.text)).toEqual(['b']);
    expect(d.filter(x => x.kind === 'add')).toEqual([]);
  });

  it('pairs a replacement as one deletion and one addition', () => {
    const d = lineDiff('pkg.Svc/Old', 'pkg.Svc/New');
    expect(d.map(x => x.kind).sort()).toEqual(['add', 'del']);
  });

  it('does not invent changes when a block moves', () => {
    // Prefix/suffix trimming would call the whole file changed here.
    const before = '--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST ---\n{}';
    const after = '--- META ---\nname: x\n\n--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST ---\n{}';
    const d = lineDiff(before, after);
    expect(d.filter(x => x.kind === 'del')).toEqual([]);
    expect(d.filter(x => x.kind === 'add').map(x => x.text)).toEqual(['--- META ---', 'name: x', '']);
  });

  it('treats empty sides as pure insert or delete', () => {
    expect(lineDiff('', 'a').map(x => x.kind)).toEqual(['add']);
    expect(lineDiff('a', '').map(x => x.kind)).toEqual(['del']);
    expect(lineDiff('', '')).toEqual([]);
  });
});
