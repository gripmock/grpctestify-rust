import { describe, expect, it } from 'vitest';
import { applyRange, applyRewrites, rewriteOf } from './text-edit';

const at = (line: number, from: number, to: number) => ({
  start: { line, character: from },
  end: { line, character: to },
});

describe('replacing what a diagnostic points at', () => {
  const text = ['--- ASSERTS ---', '.a == true && .a == true', '.b != null'].join('\n');

  it('rewrites the span and leaves the rest', () => {
    expect(applyRange(text, at(1, 0, 24), '.a == true'))
      .toBe('--- ASSERTS ---\n.a == true\n.b != null');
  });

  it('refuses a range the text does not have', () => {
    expect(applyRange(text, at(9, 0, 1), 'x')).toBeNull();
    expect(applyRange(text, at(1, 0, 999), 'x')).toBeNull();
    expect(applyRange(text, at(1, 5, 2), 'x')).toBeNull();
  });

  it('refuses a range that crosses lines — a hint is one line', () => {
    expect(applyRange(text, { start: { line: 1, character: 0 }, end: { line: 2, character: 2 } }, 'x'))
      .toBeNull();
  });
});

describe('the rewrite a hint carries', () => {
  const hint = (data?: Record<string, unknown>) => ({
    range: at(1, 0, 4),
    severity: 4,
    message: 'Optimizer hint: a -> b',
    ...(data ? { data } : {}),
  });

  it('is the replacement when there is one', () => {
    expect(rewriteOf(hint({ replacement: '.a == true' }))).toBe('.a == true');
  });

  it('is nothing otherwise', () => {
    expect(rewriteOf(hint())).toBeNull();
    expect(rewriteOf(hint({ replacement: '' }))).toBeNull();
    expect(rewriteOf(hint({ replacement: 3 }))).toBeNull();
  });
});

describe('every rewrite a file carries', () => {
  const text = ['--- OPTIONS ---', 'retry-delay: 2', 'no-retry: true', '', '!!@a', '!!@b'].join('\n');
  const key = (line: number, to: number, suggested: string) => ({
    range: { start: { line, character: 0 }, end: { line, character: to } },
    severity: 2,
    message: 'deprecated',
    data: { suggested_key: suggested },
  });
  const hint = (line: number, to: number, replacement: string) => ({
    range: { start: { line, character: 0 }, end: { line, character: to } },
    severity: 4,
    message: 'Optimizer hint',
    data: { replacement },
  });

  it('takes them all, last first, so no span moves under another', () => {
    const out = applyRewrites(text, [
      key(1, 11, 'retry_delay'),
      key(2, 8, 'no_retry'),
      hint(4, 4, '@a'),
      hint(5, 4, '@b'),
    ]);
    expect(out.applied).toBe(4);
    expect(out.text).toBe(['--- OPTIONS ---', 'retry_delay: 2', 'no_retry: true', '', '@a', '@b'].join('\n'));
  });

  it('skips a span the text cannot hold and keeps the rest', () => {
    const out = applyRewrites(text, [hint(99, 4, '@x'), hint(4, 4, '@a')]);
    expect(out.applied).toBe(1);
    expect(out.text).toContain('@a');
  });

  it('changes nothing when nothing carries a rewrite', () => {
    const out = applyRewrites(text, [{ range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } }, severity: 1, message: 'x' }]);
    expect(out).toEqual({ text, applied: 0 });
  });
});
