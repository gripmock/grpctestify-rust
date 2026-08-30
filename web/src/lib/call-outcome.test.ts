import { describe, expect, it } from 'vitest';
import { callFailed, entryFailed } from './call-outcome';

describe('whether a call failed', () => {
  it('is the gRPC rule for a gRPC call: anything but zero', () => {
    expect(callFailed({ statusCode: 0 }, false)).toBe(false);
    expect(callFailed({ statusCode: 5 }, false)).toBe(true);
  });

  it('is the HTTP rule for an HTTP call: 4xx and 5xx', () => {
    expect(callFailed({ statusCode: 200 }, true)).toBe(false);
    expect(callFailed({ statusCode: 204 }, true)).toBe(false);
    expect(callFailed({ statusCode: 301 }, true)).toBe(false);
    expect(callFailed({ statusCode: 404 }, true)).toBe(true);
    expect(callFailed({ statusCode: 500 }, true)).toBe(true);
  });

  it('is a failure whenever something went wrong before a status', () => {
    expect(callFailed({ error: 'Could not reach localhost:1' }, true)).toBe(true);
    expect(callFailed({ error: 'Connection refused', statusCode: null }, false)).toBe(true);
  });

  it('is nothing at all without a response', () => {
    expect(callFailed(null, true)).toBe(false);
    expect(callFailed({ statusCode: null }, true)).toBe(false);
  });
});

describe('a call in the history list', () => {
  const entry = (endpoint: string, response: object) => ({ endpoint, response: { status: 'ok', ...response } });

  it('is judged by the family the endpoint names', () => {
    expect(entryFailed(entry('GET /v1/users', { statusCode: 404 }))).toBe(true);
    expect(entryFailed(entry('GET /v1/users', { statusCode: 200 }))).toBe(false);
    expect(entryFailed(entry('a.B/C', { statusCode: 0 }))).toBe(false);
    expect(entryFailed(entry('a.B/C', { statusCode: 5 }))).toBe(true);
  });

  it('is failed whenever the call itself did not answer', () => {
    expect(entryFailed({ endpoint: 'GET /x', response: { status: 'error', error: 'refused' } })).toBe(true);
  });
});

describe('a run of an HTTP file', () => {
  it('is judged by its family, not by the shape of its name', () => {
    const entry = {
      endpoint: 'probe.httf',
      collectionPath: 'probe.httf',
      response: { status: 'ok', statusCode: 200, error: null },
    };
    expect(entryFailed(entry)).toBe(false);
    expect(entryFailed({ ...entry, response: { ...entry.response, statusCode: 404 } })).toBe(true);
  });

  it('still reads a gRPC status as one for a .gctf run', () => {
    expect(entryFailed({
      endpoint: 'greet.gctf',
      collectionPath: 'greet.gctf',
      response: { status: 'ok', statusCode: 5, error: null },
    })).toBe(true);
  });
});
