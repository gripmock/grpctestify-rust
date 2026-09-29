import { describe, it, expect } from 'vitest';
import { pushRecent, sortByRecency } from './history-list';

describe('pushRecent', () => {
  it('puts the newest first', () => {
    expect(pushRecent(['a'], 'b')).toEqual(['b', 'a']);
  });

  it('moves a repeat to the front instead of duplicating it', () => {
    expect(pushRecent(['a', 'b', 'c'], 'c')).toEqual(['c', 'a', 'b']);
  });

  it('ignores blank input', () => {
    expect(pushRecent(['a'], '   ')).toEqual(['a']);
  });

  it('caps the list', () => {
    const many = Array.from({ length: 20 }, (_, i) => `e${i}`);
    expect(pushRecent(many, 'new')).toHaveLength(12);
    expect(pushRecent(many, 'new')[0]).toBe('new');
  });
});

describe('sortByRecency', () => {
  it('puts the newest call first whatever order the cache hands over', () => {
    const entries = [
      { id: 'a', timestamp: 300 },
      { id: 'b', timestamp: 100 },
      { id: 'c', timestamp: 200 },
    ];
    expect(sortByRecency(entries).map(e => e.id)).toEqual(['a', 'c', 'b']);
  });

  it('leaves the input alone', () => {
    const entries = [{ id: 'a', timestamp: 1 }, { id: 'b', timestamp: 2 }];
    sortByRecency(entries);
    expect(entries.map(e => e.id)).toEqual(['a', 'b']);
  });
});
