import { describe, expect, it } from 'vitest';
import { count, plural } from './plural';

describe('a count and its noun', () => {
  it('says one of a thing in the singular', () => {
    expect(count(1, 'assert')).toBe('1 assert');
  });

  it('says any other number in the plural', () => {
    expect(count(0, 'assert')).toBe('0 asserts');
    expect(count(3, 'assert')).toBe('3 asserts');
  });

  it('takes the plural it is given when the `s` will not do', () => {
    expect(count(1, 'entry', 'entries')).toBe('1 entry');
    expect(count(4, 'entry', 'entries')).toBe('4 entries');
  });

  it('gives the noun alone where the number is written separately', () => {
    expect(plural(1, 'message')).toBe('message');
    expect(plural(2, 'message')).toBe('messages');
  });
});
