import { describe, expect, it } from 'vitest';
import { scrollForActive } from './tab-scroll';

const strip = (scrollLeft: number) => ({ scrollLeft, width: 1000, padStart: 24, padEnd: 24 });

describe('showing the active tab whole', () => {
  it('leaves a tab that is already clear of both gutters alone', () => {
    expect(scrollForActive(strip(0), { left: 100, width: 112 })).toBeNull();
  });

  it('clears the gutter on the right', () => {
    expect(scrollForActive(strip(0), { left: 940, width: 112 })).toBe(76);
  });

  it('clears the gutter on the left', () => {
    expect(scrollForActive(strip(500), { left: 510, width: 112 })).toBe(486);
  });

  it('never scrolls past the start', () => {
    expect(scrollForActive(strip(10), { left: 0, width: 112 })).toBe(0);
  });

  it('is measured against the gutters that are actually there', () => {
    const bare = { scrollLeft: 0, width: 1000, padStart: 0, padEnd: 0 };
    expect(scrollForActive(bare, { left: 888, width: 112 })).toBeNull();
  });
});
