import { describe, expect, it } from 'vitest';
import { healthNote } from './target-health';

const health = (over: Partial<Parameters<typeof healthNote>[0] & object> = {}) => ({
  reachable: true, ms: 3, detail: null, dialled: 'localhost:50051', ...over,
});

describe('what the line under the address reads', () => {
  it('says what answered, and how long the socket took', () => {
    expect(healthNote(health(), false)).toBe('something is listening on localhost:50051 — 3 ms to open a socket');
  });

  it('says what was tried when nothing answered', () => {
    expect(healthNote(health({ reachable: false, detail: 'Connection refused (os error 61)' }), false))
      .toBe('nothing answered on localhost:50051 — Connection refused (os error 61)');
  });

  it('says there was nothing to try', () => {
    expect(healthNote(health({ reachable: false, dialled: '', detail: 'no host and port to try' }), false))
      .toBe('no host and port to try');
  });

  it('says it is still trying, and nothing at all before it has', () => {
    expect(healthNote(null, true)).toBe('trying the socket…');
    expect(healthNote(null, false)).toBe('');
  });
});
