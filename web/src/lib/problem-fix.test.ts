import { describe, it, expect } from 'vitest';
import { fixFor } from './problem-fix';
import type { GctfDiagnostic } from './types';

const problem = (message: string): GctfDiagnostic => ({
  range: { start: { line: 0, character: 0 }, end: { line: 0, character: 0 } },
  severity: 1,
  message,
});

const MISSING = problem('At least one verification section (RESPONSE, ERROR, or ASSERTS) is required');
const UNVERIFIED = problem('Nothing verifies the answer yet — the run passes as long as the call succeeds. Add RESPONSE, ERROR or ASSERTS.');

describe('fixFor', () => {
  it('offers to write the section from the answer that came back', () => {
    expect(fixFor(MISSING, { hasResponse: true, failed: false })?.label).toBe('expect the answer');
  });

  it('says so when the answer was a failure', () => {
    expect(fixFor(MISSING, { hasResponse: true, failed: true })?.label).toBe('expect this failure');
  });

  it('offers nothing while there is nothing to expect', () => {
    expect(fixFor(MISSING, { hasResponse: false, failed: false })).toBeNull();
  });

  it('leaves every other problem alone', () => {
    expect(fixFor(problem('Unknown OPTIONS key "tls"'), { hasResponse: true, failed: false })).toBeNull();
  });

  it('does not promise an ERROR section to an HTTP step', () => {
    const fix = fixFor(MISSING, { hasResponse: true, failed: true, http: true });
    expect(fix?.label).toBe('expect the answer');
    expect(fix?.title).toContain('@status()');
    expect(fix?.title).not.toContain('ERROR section');
  });

  it('offers the fix to a step of a chain', () => {
    const step = problem(`Document 2: ${UNVERIFIED.message}`);
    expect(fixFor(step, { hasResponse: true, failed: false })?.id).toBe('expect-response');
  });
});

describe('the two spellings of one problem', () => {
  it('offers the same fix for what the editor says', () => {
    expect(fixFor(UNVERIFIED, { hasResponse: true, failed: false })?.id)
      .toBe(fixFor(MISSING, { hasResponse: true, failed: false })?.id);
  });
});

describe('a file with no address', () => {
  const missing = {
    range: { start: { line: 0, character: 0 }, end: { line: 0, character: 0 } },
    severity: 2,
    message: 'ADDRESS section missing — the target comes from the active environment or $GRPCTESTIFY_ADDRESS',
    data: { scope: 'file' },
  };

  it('is offered the address the calls are already going to', () => {
    expect(fixFor(missing, { hasResponse: false, failed: false, addressFromHeader: 'localhost:4770' }))
      .toMatchObject({ id: 'name-address', label: 'name localhost:4770 here' });
  });

  it('is left alone when the header is not what aims it', () => {
    expect(fixFor(missing, { hasResponse: false, failed: false, addressFromHeader: null })).toBeNull();
    expect(fixFor(missing, { hasResponse: false, failed: false, addressFromHeader: '  ' })).toBeNull();
  });
});

describe('an optimizer hint', () => {
  const hint = {
    range: { start: { line: 3, character: 0 }, end: { line: 3, character: 24 } },
    severity: 4,
    message: 'Optimizer hint: .a == true && .a == true -> .a == true',
    source: 'grpctestify-optimizer',
    data: { replacement: '.a == true' },
  };

  it('offers the rewrite it carries', () => {
    const fix = fixFor(hint, { hasResponse: false, failed: false, editable: true });
    expect(fix).toMatchObject({ id: 'apply-rewrite', label: 'rewrite it' });
    expect(fix?.title).toContain('.a == true');
  });

  it('offers nothing where an edit would land in another text', () => {
    expect(fixFor(hint, { hasResponse: false, failed: false, editable: false })).toBeNull();
  });
});

describe('the fix for a list written as a line', () => {
  const problem = {
    range: { start: { line: 1, character: 0 }, end: { line: 1, character: 20 } },
    severity: 1,
    code: 'META_LIST_EXPECTED',
    message: 'META tags is a list, not a line — write `tags: [smoke, billing]`, or one `- smoke` per line',
    data: { replacement: 'tags: [smoke, billing]' },
  };

  it('says what it writes, not which command would have', () => {
    const fix = fixFor(problem as never, { hasResponse: false, failed: false, editable: true });
    expect(fix?.id).toBe('apply-rewrite');
    expect(fix?.label).toBe('write it as a list');
    expect(fix?.title).toContain('tags: [smoke, billing]');
    expect(fix?.title).not.toContain('fmt -O');
  });

  it('is not offered over a text the edit would not land in', () => {
    expect(fixFor(problem as never, { hasResponse: false, failed: false, editable: false })).toBeNull();
  });
});

describe('who would have made this rewrite', () => {
  const at = (over: Record<string, unknown>) => ({
    range: { start: { line: 3, character: 0 }, end: { line: 3, character: 20 } },
    severity: 2,
    data: { replacement: 'thresholds.rps: >200' },
    message: 'Unknown BENCH key',
    ...over,
  }) as never;

  it('names the formatter only for its own hints', () => {
    const optimizer = fixFor(at({ message: 'Optimizer hint: @len(.a) == 0 -> @is_empty(.a)' }),
      { hasResponse: false, failed: false, editable: true });
    expect(optimizer?.title).toContain('fmt -O');
  });

  it('says what it writes for everything else', () => {
    const bench = fixFor(at({ code: 'BENCH_UNKNOWN_KEY' }), { hasResponse: false, failed: false, editable: true });
    expect(bench?.title).toBe('Replace this line with thresholds.rps: >200 — the form the runner reads');
  });
});
