import { describe, it, expect } from 'vitest';
import { collectPaths, valueAtPath , firstStringPath } from './json-paths';

describe('collectPaths', () => {
  it('walks to the leaves', () => {
    expect(collectPaths({ auth: { token: 't', user: { id: '1' } } }, 10))
      .toEqual(['.auth.token', '.auth.user.id']);
  });

  it('describes an array by its first element', () => {
    expect(collectPaths({ items: [{ id: 1 }, { id: 2 }] }, 10)).toEqual(['.items', '.items[].id']);
  });

  it('brackets keys jq cannot take bare', () => {
    expect(collectPaths({ 'user-id': 1 }, 10)).toEqual(['["user-id"]']);
  });

  it('stops at the limit', () => {
    const wide = Object.fromEntries(Array.from({ length: 20 }, (_, i) => [`k${i}`, i]));
    expect(collectPaths(wide, 4)).toHaveLength(4);
  });

  it('returns nothing for a scalar at the root', () => {
    expect(collectPaths(42, 5)).toEqual([]);
  });
});

describe('valueAtPath', () => {
  const root = { message: 'hello', user: { ids: [7, 8] }, 'odd key': 1 };

  it('walks keys, quoted keys and indexes', () => {
    expect(valueAtPath(root, '.message')).toBe('hello');
    expect(valueAtPath(root, '.user.ids[1]')).toBe(8);
    expect(valueAtPath(root, '["odd key"]')).toBe(1);
    expect(valueAtPath(root, '.')).toBe(root);
  });

  it('is undefined rather than wrong when the path misses', () => {
    expect(valueAtPath(root, '.nope')).toBeUndefined();
    expect(valueAtPath(root, '.message.deeper')).toBeUndefined();
    expect(valueAtPath(root, '.user.ids[9]')).toBeUndefined();
  });

  it('refuses anything that is not a plain path', () => {
    expect(valueAtPath(root, '.items | map(.id)')).toBeUndefined();
    expect(valueAtPath(root, 'message')).toBeUndefined();
  });
});

describe('firstStringPath', () => {
  it('finds the first text a regex could match', () => {
    expect(firstStringPath({ ok: true, auth: { token: 'tok-1' } })).toBe('.auth.token');
  });

  it('walks arrays in order', () => {
    expect(firstStringPath({ items: [{ n: 1 }, { name: 'a' }] })).toBe('.items[1].name');
  });

  it('quotes a key that is not an identifier', () => {
    expect(firstStringPath({ 'x-id': 'v' })).toBe('.["x-id"]');
  });

  it('answers null when there is no text in it', () => {
    expect(firstStringPath({ a: 1, b: [2, 3] })).toBeNull();
    expect(firstStringPath('a string')).toBeNull();
  });
});
