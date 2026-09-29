import { describe, it, expect } from 'vitest';
import { answered, seedIndex, seedLabel, seedMessage } from './response-seed';
import type { CallResult } from './types';

function result(messages: unknown[], status: CallResult['status'] = 'ok'): CallResult {
  return { status, statusCode: 0, messages, headers: {}, trailers: {}, error: null, durationMs: 1 };
}

describe('seedIndex', () => {
  it('clamps to what came back', () => {
    expect(seedIndex(result([{ a: 1 }, { a: 2 }]), 1)).toBe(1);
    expect(seedIndex(result([{ a: 1 }, { a: 2 }]), 9)).toBe(1);
    expect(seedIndex(result([{ a: 1 }]), -3)).toBe(0);
    expect(seedIndex(result([]), 2)).toBe(0);
    expect(seedIndex(null, 2)).toBe(0);
  });
});

describe('seedMessage', () => {
  it('is the selected message, not always the first', () => {
    expect(seedMessage(result([{ a: 1 }, { a: 2 }, { a: 3 }]), 2)).toEqual({ a: 3 });
  });

  it('is nothing when there is nothing to chew on', () => {
    expect(seedMessage(null, 0)).toBeNull();
    expect(seedMessage(result([]), 0)).toBeNull();
    expect(seedMessage(result([], 'error'), 0)).toBeNull();
    expect(seedMessage(result([{ a: 1 }], 'pending'), 0)).toBeNull();
  });

  it('is the message a failed run came back with', () => {
    expect(seedMessage(result([{ a: 1 }], 'error'), 0)).toEqual({ a: 1 });
  });
});

describe('answered', () => {
  it('is what came back, whatever the verdict on it', () => {
    expect(answered(result([{ a: 1 }]))).toBe(true);
    expect(answered(result([{ a: 1 }], 'error'))).toBe(true);
    expect(answered(result([], 'error'))).toBe(false);
    expect(answered(result([{ a: 1 }], 'pending'))).toBe(false);
    expect(answered(null)).toBe(false);
  });
});

describe('seedLabel', () => {
  it('says which one only when there is a choice', () => {
    expect(seedLabel(result([{ a: 1 }]), 0)).toBeNull();
    expect(seedLabel(result([{ a: 1 }, { a: 2 }, { a: 3 }]), 1)).toBe('message 2 of 3');
    expect(seedLabel(result([{ a: 1 }, { a: 2 }]), 7)).toBe('message 2 of 2');
  });
});
