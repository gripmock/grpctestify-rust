import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest';
import { drop, readJson, readNumber, readText, writeJson, writeText } from 'luvo/data/storage';

function memory() {
  const held = new Map<string, string>();
  vi.stubGlobal('localStorage', {
    getItem: (k: string) => (held.has(k) ? held.get(k)! : null),
    setItem: (k: string, v: string) => { held.set(k, String(v)); },
    removeItem: (k: string) => { held.delete(k); },
  });
}

function refuse() {
  const thrower = () => { throw new DOMException('The operation is insecure.'); };
  vi.stubGlobal('localStorage', { getItem: thrower, setItem: thrower, removeItem: thrower });
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('a store that refuses', () => {
  it('answers with the fallback rather than throwing', () => {
    refuse();
    expect(readText('k', 'fallback')).toBe('fallback');
    expect(readNumber('k', 7, 0, 10)).toBe(7);
    expect(readJson('k', { a: 1 })).toEqual({ a: 1 });
  });

  it('says a write did not happen', () => {
    refuse();
    expect(writeText('k', 'v')).toBe(false);
    expect(writeJson('k', { a: 1 })).toBe(false);
  });

  it('drops without complaint', () => {
    refuse();
    expect(() => drop('k')).not.toThrow();
  });
});

describe('a store that holds nonsense', () => {
  beforeEach(memory);

  it('keeps a number inside the range that means something', () => {
    writeText('n', '999999');
    expect(readNumber('n', 380, 220, 900)).toBe(900);
    writeText('n', '-4');
    expect(readNumber('n', 380, 220, 900)).toBe(220);
  });

  it('falls back on what is not a number at all', () => {
    writeText('n', 'wide');
    expect(readNumber('n', 380, 220, 900)).toBe(380);
    writeText('n', '');
    expect(readNumber('n', 380, 220, 900)).toBe(380);
  });

  it('falls back on JSON that will not parse, and on a null record', () => {
    writeText('j', '{"half":');
    expect(readJson('j', [])).toEqual([]);
    writeText('j', 'null');
    expect(readJson('j', [1])).toEqual([1]);
  });

  it('round-trips what it was given', () => {
    expect(writeJson('j', { a: [1, 2] })).toBe(true);
    expect(readJson('j', null)).toEqual({ a: [1, 2] });
  });
});
