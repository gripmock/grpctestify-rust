import { describe, it, expect } from 'vitest';
import { expectMode, errorExpectBody, expectDisagreement, disagreementNote, expectBody, numbersRounded } from './expect-model';
import type { CollectionParsed } from './types';

const base = { expect_responses: [], expect_error: null } as unknown as CollectionParsed;
const message = { body: '{}', partial: false, unordered_arrays: false, with_asserts: false, tolerance: null, redact: [] };

describe('expectMode', () => {
  it('is asserts-only when the file states no outcome', () => {
    expect(expectMode(base)).toBe('none');
    expect(expectMode(null)).toBe('none');
  });

  it('follows RESPONSE and ERROR', () => {
    expect(expectMode({ ...base, expect_responses: [message] })).toBe('response');
    expect(expectMode({ ...base, expect_error: message })).toBe('error');
  });

  it('answers error when a file carries both', () => {
    expect(expectMode({ ...base, expect_responses: [message], expect_error: message })).toBe('error');
  });
});

describe('errorExpectBody', () => {
  it('writes the code as the number the runner compares', () => {
    expect(JSON.parse(errorExpectBody(5, 'No matching stub found'))).toEqual({
      code: 5, message: 'No matching stub found',
    });
  });

  it('leaves out a code that says nothing', () => {
    expect(JSON.parse(errorExpectBody(0, 'boom'))).toEqual({ message: 'boom' });
    expect(JSON.parse(errorExpectBody(null, 'boom'))).toEqual({ message: 'boom' });
  });

  it('still writes an expectation for a failure with no words', () => {
    expect(JSON.parse(errorExpectBody(null, '   '))).toEqual({});
  });
});

describe('what the file expects against what came back', () => {
  it('says so when the file expects a failure and the call succeeded', () => {
    expect(expectDisagreement('error', { failed: false })).toBe('expects-failure-got-ok');
    expect(disagreementNote('expects-failure-got-ok')).toContain('expects the call to fail');
  });

  it('says so the other way round', () => {
    expect(expectDisagreement('response', { failed: true })).toBe('expects-messages-got-failure');
  });

  it('is quiet when they agree', () => {
    expect(expectDisagreement('error', { failed: true })).toBeNull();
    expect(expectDisagreement('response', { failed: false })).toBeNull();
  });

  it('is quiet for asserts, and before anything has been sent', () => {
    expect(expectDisagreement('none', { failed: true })).toBeNull();
    expect(expectDisagreement('error', null)).toBeNull();
    expect(disagreementNote(null)).toBeNull();
  });
});

describe('the body an expectation takes from the answer', () => {
  it('keeps text as text and writes anything else as JSON', () => {
    expect(expectBody('plain words here')).toBe('plain words here');
    expect(expectBody({ id: 'u-1' })).toBe('{\n  "id": "u-1"\n}');
    expect(expectBody([1, 2])).toBe('[\n  1,\n  2\n]');
  });
});

describe('an expectation written from an answer', () => {
  it('keeps a number the round trip would change', () => {
    const raw = '{"id": 9007199254740993}';
    const parsed = JSON.parse(raw);
    expect(expectBody(parsed, raw)).toBe(raw);
  });

  it('pretty-prints what does survive the round trip', () => {
    const raw = '{"id":7,"name":"Ada"}';
    expect(expectBody(JSON.parse(raw), raw)).toBe('{\n  "id": 7,\n  "name": "Ada"\n}');
  });

  it('falls back to the parsed message when no text came with it', () => {
    expect(expectBody({ a: 1 })).toBe('{\n  "a": 1\n}');
  });

  it('leaves a text answer as the text it is', () => {
    expect(expectBody('plain words', 'plain words')).toBe('plain words');
  });
});

describe('numbers the panel cannot show exactly', () => {
  it('are recognised in the text that came back', () => {
    expect(numbersRounded('{"id": 9007199254740993}')).toBe(true);
    expect(numbersRounded('{"id": 9007199254740992}')).toBe(false);
    expect(numbersRounded('{"id": 7}')).toBe(false);
    expect(numbersRounded(undefined)).toBe(false);
  });
});
