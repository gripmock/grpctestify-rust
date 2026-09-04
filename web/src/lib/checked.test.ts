import { describe, expect, it } from 'vitest';
import { checkMark, checkSummary, checkedAfterMove, mergeChecked, rollUpChecks } from './checked';

describe('the mark a checked file carries', () => {
  it('is nothing for a file with nothing to say', () => {
    expect(checkMark(undefined)).toBeNull();
    expect(checkMark({ path: 'a.gctf', errors: 0, warnings: 0 })).toBeNull();
  });

  it('counts errors before warnings, and says the first of them', () => {
    expect(checkMark({ path: 'a.gctf', errors: 2, warnings: 3, first: 'no ENDPOINT' }))
      .toEqual({ kind: 'error', label: '2', title: '2 errors — no ENDPOINT' });
  });

  it('is a warning where there are only warnings', () => {
    expect(checkMark({ path: 'a.gctf', errors: 0, warnings: 1, first: 'ADDRESS section missing' }))
      .toEqual({ kind: 'warn', label: '1', title: '1 warning — ADDRESS section missing' });
  });
});

describe('what a check of a set came to', () => {
  it('says so when there is nothing to report', () => {
    expect(checkSummary([], 44, false)).toBe('44 files checked — nothing to report');
  });

  it('counts both kinds and the files they are in', () => {
    expect(checkSummary([
      { path: 'a', errors: 1, warnings: 1 },
      { path: 'b', errors: 0, warnings: 2 },
    ], 44, false)).toBe('1 error · 3 warnings in 2 of 44 files');
  });

  it('says when the set was cut', () => {
    expect(checkSummary([{ path: 'a', errors: 1, warnings: 0 }], 500, true))
      .toBe('1 error in 1 of 500 files — the first 500');
  });
});

describe('the marks after another check', () => {
  const a = { path: 'a.gctf', errors: 1, warnings: 0 };
  const b = { path: 'b.gctf', errors: 0, warnings: 2 };

  it('drops what was asked about and came back clean', () => {
    expect(mergeChecked({ 'a.gctf': a, 'b.gctf': b }, ['a.gctf'], [])).toEqual({ 'b.gctf': b });
  });

  it('replaces what came back with something to say', () => {
    const fresh = { path: 'a.gctf', errors: 0, warnings: 1 };
    expect(mergeChecked({ 'a.gctf': a }, ['a.gctf'], [fresh])).toEqual({ 'a.gctf': fresh });
  });

  it('leaves files nobody asked about alone', () => {
    expect(mergeChecked({ 'b.gctf': b }, ['a.gctf'], [])).toEqual({ 'b.gctf': b });
  });
});

describe('the marks after a file moves', () => {
  const a = { path: 'old/a.gctf', errors: 1, warnings: 0 };

  it('move with it, path and all', () => {
    expect(checkedAfterMove({ 'old/a.gctf': a }, 'old/a.gctf', 'new/a.gctf'))
      .toEqual({ 'new/a.gctf': { ...a, path: 'new/a.gctf' } });
  });

  it('follow a folder that moved', () => {
    expect(checkedAfterMove({ 'old/a.gctf': a }, 'old', 'kept'))
      .toEqual({ 'kept/a.gctf': { ...a, path: 'kept/a.gctf' } });
  });

  it('go when the file went', () => {
    expect(checkedAfterMove({ 'old/a.gctf': a }, 'old/a.gctf', null)).toEqual({});
  });
});

describe('what a folder full of checked files comes to', () => {
  const checked = {
    'a/one.gctf': { path: 'a/one.gctf', errors: 1, warnings: 2 },
    'a/two.gctf': { path: 'a/two.gctf', errors: 0, warnings: 1 },
    'b/three.gctf': { path: 'b/three.gctf', errors: 5, warnings: 0 },
  };

  it('counts the files and both kinds inside it', () => {
    expect(rollUpChecks(['a/one.gctf', 'a/two.gctf'], checked))
      .toEqual({ files: 2, errors: 1, warnings: 3 });
  });

  it('is nothing where nothing inside was checked', () => {
    expect(rollUpChecks(['c/four.gctf'], checked)).toEqual({ files: 0, errors: 0, warnings: 0 });
  });
});
