import { describe, it, expect } from 'vitest';
import { arrivals, isNotable, NOTABLE_GAP_MS } from './stream-timing';

describe('when the messages of a stream arrived', () => {
  it('keeps the transport offset and derives the wait', () => {
    expect(arrivals([2, 2, 500, 501])).toEqual([
      { at: 2, gap: null },
      { at: 2, gap: 0 },
      { at: 500, gap: 498 },
      { at: 501, gap: 1 },
    ]);
  });

  it('has nothing to say about an empty stream', () => {
    expect(arrivals([])).toEqual([]);
  });

  it('never reports a negative wait', () => {
    expect(arrivals([10, 4])[1].gap).toBe(0);
  });

  it('calls a pause notable only when it is one', () => {
    expect(isNotable(null)).toBe(false);
    expect(isNotable(NOTABLE_GAP_MS - 1)).toBe(false);
    expect(isNotable(NOTABLE_GAP_MS)).toBe(true);
  });
});
