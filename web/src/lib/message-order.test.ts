import { describe, it, expect } from 'vitest';
import { duplicateItem, moveItem } from './message-order';

describe('moveItem', () => {
  it('moves one up and one down', () => {
    expect(moveItem(['a', 'b', 'c'], 2, 1)).toEqual(['a', 'c', 'b']);
    expect(moveItem(['a', 'b', 'c'], 0, 2)).toEqual(['b', 'c', 'a']);
  });

  it('does nothing at the edges, or for a move to itself', () => {
    const items = ['a', 'b'];
    expect(moveItem(items, 0, -1)).toBe(items);
    expect(moveItem(items, 1, 2)).toBe(items);
    expect(moveItem(items, 1, 1)).toBe(items);
  });
});

describe('duplicateItem', () => {
  it('puts the copy right after its original', () => {
    expect(duplicateItem(['a', 'b'], 0)).toEqual(['a', 'a', 'b']);
    expect(duplicateItem(['a', 'b'], 1)).toEqual(['a', 'b', 'b']);
  });

  it('refuses an index that is not there', () => {
    const items = ['a'];
    expect(duplicateItem(items, 3)).toBe(items);
  });
});
