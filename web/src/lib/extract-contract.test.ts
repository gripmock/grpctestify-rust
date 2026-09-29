import { describe, it, expect } from 'vitest';
import { extractAudience, extractAudienceEmpty, extractionInput, flowLabel, flowTitle, previewSource, ranValue, reachLabel, reachOf } from './extract-contract';

const docs = [
  { consumes: [] },
  { consumes: ['token'] },
  { consumes: ['token', 'id'] },
] as any;

describe('who reads an extracted name', () => {
  it('is the later steps that use it', () => {
    expect(reachOf('token', docs, 0)).toEqual({ kind: 'steps', steps: [1, 2] });
    expect(reachOf('id', docs, 0)).toEqual({ kind: 'steps', steps: [2] });
  });

  it('never counts the step doing the extraction or one before it', () => {
    expect(reachOf('token', docs, 1)).toEqual({ kind: 'steps', steps: [2] });
    expect(reachOf('token', docs, 2)).toEqual({ kind: 'none' });
  });

  it('is this document\'s own asserts when no later step reads it', () => {
    expect(reachOf('item_count', docs, 0, ['item_count == 2'])).toEqual({ kind: 'asserts' });
    expect(reachOf('count', docs, 0, ['item_count == 2'])).toEqual({ kind: 'none' });
    expect(reachOf('token', docs, 0, ['token != ""'])).toEqual({ kind: 'steps', steps: [1, 2] });
  });

  it('reads a name inside a placeholder too', () => {
    expect(reachOf('id', docs, 2, ['.body == "{{id}}"'])).toEqual({ kind: 'asserts' });
  });

  it('says so in words', () => {
    expect(reachLabel({ kind: 'none' })).toBe('unread');
    expect(reachLabel({ kind: 'asserts' })).toBe('asserts');
    expect(reachLabel({ kind: 'steps', steps: [1, 2] })).toBe('→ step 2, step 3');
  });
});

describe('the message an extraction reads', () => {
  it('is the last one, as the runner takes it', () => {
    expect(extractionInput([{ a: 1 }, { a: 2 }])).toEqual({ message: { a: 2 }, index: 1, total: 2 });
  });

  it('is nothing when there is no response', () => {
    expect(extractionInput([])).toBeNull();
  });
});

describe('the response a preview may read', () => {
  const ok = (fromStep?: number) => ({ status: 'ok', messages: [{}], fromStep });

  it('is this step\'s own response', () => {
    expect(previewSource(ok(1), 1, 1)).toEqual({ ok: true, note: 'What these take from the last response' });
  });

  it('names the step a stale response came from', () => {
    expect(previewSource(ok(0), 1, 1)).toEqual({
      ok: false,
      note: 'The response on screen is from step 1 — execute this step to check these',
    });
  });

  it('will not read a whole-file run as a step', () => {
    expect(previewSource(ok(undefined), 1, 1).ok).toBe(false);
  });

  it('says which message of a stream it reads', () => {
    expect(previewSource(ok(0), 0, 3).note).toContain('message 3 of 3');
  });

  it('has nothing to read without a response', () => {
    expect(previewSource(null, 0, 0).ok).toBe(false);
    expect(previewSource({ status: 'error', messages: [], fromStep: 0 }, 0, 0).ok).toBe(false);
  });
});

describe('who would read an extraction', () => {
  it('is the steps after this one, when there are any', () => {
    expect(extractAudience(3, 0)).toBe('later steps read it as {{name}}');
    expect(extractAudienceEmpty(3, 1)).toBe('later steps read them as {{name}}');
  });

  it('is nobody, in the single-document file most tests are', () => {
    expect(extractAudience(1)).toContain('nothing reads it yet');
    expect(extractAudienceEmpty(1)).toContain('because this file ends here');
  });

  it('is nobody for the last step of a chain either', () => {
    expect(extractAudience(3, 2)).toContain('nothing reads it yet');
  });
});

describe('what a run left behind', () => {
  it('says the values are the file\'s own when there is no answer on screen', () => {
    expect(previewSource(null, 0, 0, true).note).toBe('What this file has bound');
    expect(previewSource(null, 0, 0, false).note).toContain('Execute this step');
  });

  it('still prefers the answer on screen when there is one', () => {
    const answered = { status: 'ok', messages: [{ id: 7 }], fromStep: 0 };
    expect(previewSource(answered, 0, 1, true).ok).toBe(true);
  });

  it('reads one name out of what the run bound', () => {
    const ran: [string, string][] = [['user', '7'], ['token', 'abc']];
    expect(ranValue(ran, 'token')).toBe('abc');
    expect(ranValue(ran, 'missing')).toBeNull();
    expect(ranValue(undefined, 'user')).toBeNull();
  });
});

describe('what a variable says on the connector', () => {
  it('carries the value once a run has bound one', () => {
    expect(flowLabel('user', '7')).toBe('user = 7');
    expect(flowTitle('user', '7')).toContain('carried forward');
  });

  it('is the name alone until then', () => {
    expect(flowLabel('user', null)).toBe('user');
    expect(flowTitle('user', null)).toContain('read by a later step');
  });

  it('keeps a long value out of the rail', () => {
    const token = 'eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9';
    expect(flowLabel('token', token)).toBe('token');
    expect(flowTitle('token', token)).toContain(token);
  });

  it('reads a value written across lines as one line', () => {
    expect(flowLabel('id', ' 7\n')).toBe('id = 7');
  });
});
