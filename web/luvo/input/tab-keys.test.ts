import { describe, it, expect } from 'vitest';
import { nextTabIndex, dropIndex } from 'luvo/input/tab-keys';

describe('nextTabIndex', () => {
  it('walks both ways', () => {
    expect(nextTabIndex(1, 4, 'ArrowRight')).toBe(2);
    expect(nextTabIndex(1, 4, 'ArrowLeft')).toBe(0);
  });

  it('wraps, because a strip is a ring', () => {
    expect(nextTabIndex(3, 4, 'ArrowRight')).toBe(0);
    expect(nextTabIndex(0, 4, 'ArrowLeft')).toBe(3);
  });

  it('goes to the ends', () => {
    expect(nextTabIndex(2, 4, 'Home')).toBe(0);
    expect(nextTabIndex(2, 4, 'End')).toBe(3);
  });

  it('leaves keys it does not own alone', () => {
    expect(nextTabIndex(2, 4, 'Enter')).toBeNull();
    expect(nextTabIndex(2, 4, 'a')).toBeNull();
  });

  it('has nowhere to go in an empty strip', () => {
    expect(nextTabIndex(0, 0, 'ArrowRight')).toBeNull();
  });
});

describe('moving a tab', () => {
  it('drops before the tab under the pointer, or after it past the middle', () => {
    expect(dropIndex(2, false)).toBe(2);
    expect(dropIndex(2, true)).toBe(3);
  });
});
