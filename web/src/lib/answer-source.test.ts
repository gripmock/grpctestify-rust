import { describe, expect, it } from 'vitest';
import { serverAnswered } from './answer-source';
import type { CallResult } from './types';

const result = (over: Partial<CallResult>): CallResult => ({
  status: 'ok', statusCode: 0, messages: [], headers: {}, trailers: {},
  error: null, durationMs: 1, ...over,
});

describe('whether the server ever answered', () => {
  it('is a status code, whatever it says', () => {
    expect(serverAnswered(result({}))).toBe(true);
    expect(serverAnswered(result({ status: 'error', statusCode: 5, error: 'not found' }))).toBe(true);
    expect(serverAnswered(result({ status: 'error', statusCode: 500 }))).toBe(true);
  });

  it('is not a call that never arrived', () => {
    expect(serverAnswered(result({
      status: 'error', statusCode: null, error: 'Could not reach localhost:59999: Connection refused',
    }))).toBe(false);
    expect(serverAnswered(result({ status: 'error', statusCode: null, sent: false, error: 'no address' }))).toBe(false);
    expect(serverAnswered(null)).toBe(false);
    expect(serverAnswered(result({ status: 'pending' }))).toBe(false);
  });

  it('is an empty stream that came back', () => {
    expect(serverAnswered(result({ statusCode: 0, messages: [] }))).toBe(true);
  });

  it('is a message with no code beside it', () => {
    expect(serverAnswered(result({ statusCode: null, messages: [{ a: 1 }] }))).toBe(true);
  });
});
