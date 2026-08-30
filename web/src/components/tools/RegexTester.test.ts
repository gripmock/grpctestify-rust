import { describe, it, expect } from 'vitest';
import { extractLines, withInlineFlags } from './RegexTester';

describe('withInlineFlags', () => {
  it('leaves a pattern alone when no flag is set', () => {
    expect(withInlineFlags('^ada$', '')).toBe('^ada$');
  });

  it('carries the flags the tester ran with into the pattern', () => {
    expect(withInlineFlags('^ADA$', 'i')).toBe('(?i)^ADA$');
    expect(withInlineFlags('^a.*$', 'is')).toBe('(?is)^a.*$');
  });

  it('drops what this engine has no flag for', () => {
    expect(withInlineFlags('^ada$', 'ig')).toBe('(?i)^ada$');
    expect(withInlineFlags('^ada$', 'g')).toBe('^ada$');
  });
});

describe('the EXTRACT a pattern is worth', () => {
  it('binds every named group, through the field the match was read from', () => {
    expect(extractLines('tok-(?<id>[a-f0-9]+)', '.header', [['id', '8f3a']]))
      .toEqual([['id', '.header | capture("tok-(?<id>[a-f0-9]+)").id']]);
  });

  it('leaves out a group jq only numbers — `.1` is not a path', () => {
    expect(extractLines('(a)(?<b>c)', '.m', [['1', 'a'], ['b', 'c']]).map(([name]) => name))
      .toEqual(['b']);
  });

  it('quotes a pattern that would otherwise end the string', () => {
    expect(extractLines('"(?<q>x)"', '.m', [['q', 'x']])[0]![1])
      .toBe('.m | capture("\\"(?<q>x)\\"").q');
  });

  it('is nothing when nothing was captured', () => {
    expect(extractLines('plain', '.m', [])).toEqual([]);
  });
});
