import { describe, it, expect } from 'vitest';
import { metaEmptyNote, outcomeBadge } from './outcome';

const ok = (messages: number) => ({ status: 'ok' as const, messages: Array(messages).fill({}) });

describe('outcomeBadge', () => {
  it('counts what came back', () => {
    expect(outcomeBadge(ok(1)).label).toBe('ok · 1 msg');
    expect(outcomeBadge(ok(3)).label).toBe('ok · 3 msgs');
  });

  it('says a call is still going', () => {
    expect(outcomeBadge({ status: 'pending', messages: [] }).kind).toBe('pending');
  });

  it('tells a failed check from a failed call', () => {
    const badge = outcomeBadge({
      status: 'error',
      messages: [{}],
      assertions: [{ passed: false }, { passed: true }],
    });
    expect(badge.kind).toBe('checks');
    expect(badge.label).toBe('checks failed · 1/2');
  });

  it('is an error when nothing came back at all', () => {
    expect(outcomeBadge({ status: 'error', messages: [], assertions: [{ passed: false }] }).kind).toBe('error');
    expect(outcomeBadge({ status: 'error', messages: [] }).kind).toBe('error');
  });

  it('is an error when every check passed and the call failed anyway', () => {
    expect(outcomeBadge({ status: 'error', messages: [{}], assertions: [{ passed: true }] }).kind).toBe('error');
  });
});

describe('an HTTP answer that is a failure', () => {
  const answered = (statusCode: number) =>
    outcomeBadge({ status: 'ok', messages: [{}], statusCode }, true);

  it('does not read as ok beside a red status', () => {
    expect(answered(404).kind).toBe('error');
    expect(answered(500).label).toContain('answered');
  });

  it('still reads as ok for everything below 400', () => {
    expect(answered(200).kind).toBe('ok');
    expect(answered(204).label).toBe('ok');
    expect(answered(302).kind).toBe('ok');
  });

  it('is not counted in messages', () => {
    expect(outcomeBadge({ status: 'ok', messages: [], statusCode: 204 }, true).label).toBe('ok');
    expect(outcomeBadge({ status: 'ok', messages: [{}], statusCode: 200 }, true).label).toBe('ok');
    expect(outcomeBadge({ status: 'ok', messages: [{}], statusCode: 404 }, true).label).toBe('answered');
  });

  it('leaves a gRPC call alone, where 200 is not a status at all', () => {
    expect(outcomeBadge({ status: 'ok', messages: [{}], statusCode: 0 }).kind).toBe('ok');
  });
});

describe('a call that answered and then failed', () => {
  it('says how much came back before it did', () => {
    const badge = outcomeBadge({ status: 'error', messages: [{ status: 'SERVING' }], statusCode: 1 });
    expect(badge).toMatchObject({ kind: 'error', label: 'error · 1 msg' });
    expect(badge.title).toBe('1 msg came back before the call failed — they are below');
  });

  it('counts them', () => {
    expect(outcomeBadge({ status: 'error', messages: [1, 2, 3] }).label).toBe('error · 3 msgs');
  });

  it('says nothing of the kind when nothing came back', () => {
    expect(outcomeBadge({ status: 'error', messages: [] }).label).toBe('error');
  });

  it('leaves an HTTP failure alone — one request, one answer', () => {
    expect(outcomeBadge({ status: 'error', messages: [{ a: 1 }] }, true).label).toBe('error');
  });
});

describe('what a passing run left behind', () => {
  it('does not count the messages of a record that holds none', () => {
    const badge = outcomeBadge({ status: 'ok', messages: [], fromRun: true });
    expect(badge.label).toBe('ok');
    expect(badge.title).toContain('keeps no body');
  });

  it('counts them when the run did keep them', () => {
    expect(outcomeBadge({ status: 'ok', messages: [{ a: 1 }], fromRun: true }).label).toBe('ok · 1 msg');
  });

  it('leaves a call made here alone — no answer is news there', () => {
    expect(outcomeBadge({ status: 'ok', messages: [] }).label).toBe('ok · 0 msgs');
  });
});

describe('a request that never left', () => {
  it('is not an error on the wire', () => {
    const badge = outcomeBadge({ status: 'error', messages: [], sent: false });
    expect(badge.kind).toBe('refused');
    expect(badge.label).toBe('not sent');
  });

  it('says nothing about a target it never dialled', () => {
    expect(outcomeBadge({ status: 'error', messages: [], statusCode: 5, sent: false }).label).toBe('not sent');
  });

  it('leaves a call that did go out alone', () => {
    expect(outcomeBadge({ status: 'error', messages: [], sent: true }).kind).toBe('error');
    expect(outcomeBadge({ status: 'error', messages: [] }).kind).toBe('error');
  });
});

describe('an empty headers tab', () => {
  it('says the run kept none rather than that the server sent none', () => {
    expect(metaEmptyNote(true, true)).toBe(
      "The run kept this file's checks, not its answer — Execute to see the headers.",
    );
  });

  it('says metadata to a gRPC reader and headers to an HTTP one', () => {
    expect(metaEmptyNote(false, true)).toContain('metadata');
    expect(metaEmptyNote(true, false)).toContain('headers');
  });

  it('keeps the plain reading for a call made here', () => {
    expect(metaEmptyNote(false, false)).toBe('The server sent no metadata with this response.');
  });
});
