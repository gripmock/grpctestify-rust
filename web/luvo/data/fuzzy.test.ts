import { describe, it, expect } from 'vitest';
import { rankPaths, scorePath } from 'luvo/data/fuzzy';

const SUITE = [
  'auth/login.gctf',
  'auth/logout.gctf',
  'billing/invoice.gctf',
  'catalog/items/bulk-import.gctf',
  'catalog/items/create.gctf',
  'users/profile/get.gctf',
];

describe('scorePath', () => {
  it('ranks what was typed in the name above an accident of letters', () => {
    const login = scorePath('auth/login.gctf', 'log')!;
    const catalog = scorePath('catalog/items/create.gctf', 'log')!;
    expect(login).toBeGreaterThan(catalog);
  });

  it('ranks a name that is only what was typed highest', () => {
    expect(scorePath('a/get.gctf', 'get')!).toBeGreaterThan(scorePath('a/get-user.gctf', 'get')!);
  });

  it('ranks a hit in the name above one in a folder', () => {
    expect(scorePath('x/login.gctf', 'log')!).toBeGreaterThan(scorePath('login/x.gctf', 'log')!);
  });

  /* `cip` for `catalog/items/pricing` is a real way to type. */
  it('keeps letters-in-order matching, below a substring', () => {
    expect(scorePath('catalog/items/pricing.gctf', 'cip')).not.toBeNull();
    expect(scorePath('catalog/items/pricing.gctf', 'cip')!)
      .toBeLessThan(scorePath('catalog/items/pricing.gctf', 'pricing')!);
  });

  it('scores a tight run above a scattered one', () => {
    expect(scorePath('abc-zzzzzzzz.gctf', 'abc')!).toBeGreaterThan(scorePath('a/b/zzzzzzzzz/c.gctf', 'abc')!);
  });

  it('is no match when a letter is missing', () => {
    expect(scorePath('auth/login.gctf', 'zq')).toBeNull();
  });

  it('matches everything on an empty query', () => {
    expect(scorePath('anything.gctf', '  ')).toBe(0);
  });
});

describe('rankPaths', () => {
  it('puts the two files anyone meant by "log" first', () => {
    expect(rankPaths(SUITE, 'log').slice(0, 2)).toEqual(['auth/login.gctf', 'auth/logout.gctf']);
  });

  it('drops what does not match at all', () => {
    expect(rankPaths(SUITE, 'invoice')).toEqual(['billing/invoice.gctf']);
  });

  /* Two keystrokes must not reshuffle equal matches under the cursor. */
  it('keeps the input order between ties', () => {
    expect(rankPaths(['b/x.gctf', 'a/x.gctf'], 'x.gctf')).toEqual(['b/x.gctf', 'a/x.gctf']);
  });
});
