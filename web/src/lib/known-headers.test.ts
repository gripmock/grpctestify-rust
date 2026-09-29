import { describe, expect, it } from 'vitest';
import { knownHeaders } from './known-headers';

describe('the header names offered while one is typed', () => {
  it('are the ones that family carries', () => {
    expect(knownHeaders('http')).toContain('content-type');
    expect(knownHeaders('http')).not.toContain('grpc-timeout');
    expect(knownHeaders('grpc')).toContain('grpc-timeout');
    expect(knownHeaders('grpc')).not.toContain('user-agent');
  });

  it('are lowercase, sorted and unique', () => {
    for (const wire of ['http', 'grpc'] as const) {
      const names = knownHeaders(wire);
      expect(names).toEqual([...names].map(n => n.toLowerCase()));
      expect(names).toEqual([...names].sort());
      expect(new Set(names).size).toBe(names.length);
    }
  });
});
