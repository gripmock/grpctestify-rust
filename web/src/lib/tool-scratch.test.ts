import { describe, it, expect, beforeEach } from 'vitest';
import { forgetScratch, keep, keptValue } from './tool-scratch';

describe('what the drawer keeps', () => {
  beforeEach(() => forgetScratch());

  it('answers with what was kept, not with the initial value again', () => {
    expect(keptValue('jq.expr', () => '.')).toBe('.');
    keep('jq.expr', '.items | length');
    expect(keptValue('jq.expr', () => '.')).toBe('.items | length');
  });

  it('keeps a value that is falsey, rather than seeding over it', () => {
    keep('regex.flags', '');
    expect(keptValue('regex.flags', () => 'i')).toBe('');
  });

  it('keeps one key apart from another', () => {
    keep('jq.expr', '.a');
    keep('regex.pattern', 'tok-');
    expect(keptValue('jq.expr', () => '')).toBe('.a');
    expect(keptValue('regex.pattern', () => '')).toBe('tok-');
  });

  it('starts again once the window has thrown it away', () => {
    keep('jq.pasted', '{"a":1}');
    forgetScratch();
    expect(keptValue('jq.pasted', () => '{}')).toBe('{}');
  });
});
