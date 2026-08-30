import { describe, it, expect } from 'vitest';
import { droppedNames, droppedQuestion } from './env-drop';

const use = (name: string, files: string[]) => ({ name, files, count: files.length });

describe('names an environment stops defining', () => {
  it('finds the ones files read', () => {
    const dropped = droppedNames(['TOKEN', 'USER'], ['USER'], [use('TOKEN', ['a.gctf']), use('OTHER', ['b.gctf'])]);
    expect(dropped.map(d => d.name)).toEqual(['TOKEN']);
  });

  it('says nothing about a name nothing reads', () => {
    expect(droppedNames(['SPARE'], [], [use('TOKEN', ['a.gctf'])])).toEqual([]);
  });

  it('says nothing when the name is still there', () => {
    expect(droppedNames(['TOKEN'], ['TOKEN', 'NEW'], [use('TOKEN', ['a.gctf'])])).toEqual([]);
  });

  it('names the files, and counts the rest', () => {
    const q = droppedQuestion([use('TOKEN', ['a.gctf', 'b.gctf', 'c.gctf', 'd.gctf'])]);
    expect(q).toContain('{{TOKEN}}');
    expect(q).toContain('a.gctf, b.gctf, c.gctf and 1 more');
  });
});
