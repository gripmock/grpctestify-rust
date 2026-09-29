import { describe, expect, it } from 'vitest';
import { collapsesAt, nextStop, snap } from './snap';

const RAIL = [200, 260, 340, 440] as const;

describe('snap', () => {
  it('sticks inside the magnet and drags freely outside it', () => {
    expect(snap(255, RAIL)).toBe(260);
    expect(snap(300, RAIL)).toBe(300);
  });

  it('takes the nearest stop when two are in range', () => {
    expect(snap(232, [230, 240])).toBe(230);
    expect(snap(238, [230, 240])).toBe(240);
  });

  it('does nothing when snapping is suppressed', () => {
    expect(snap(255, RAIL, 12, false)).toBe(255);
  });
});

describe('nextStop', () => {
  it('cycles upward and wraps', () => {
    expect(nextStop(200, RAIL)).toBe(260);
    expect(nextStop(440, RAIL)).toBe(200);
  });
});

describe('collapsesAt', () => {
  it('collapses only well below the smallest stop', () => {
    expect(collapsesAt(150, RAIL)).toBe(true);
    expect(collapsesAt(190, RAIL)).toBe(false);
  });
});
