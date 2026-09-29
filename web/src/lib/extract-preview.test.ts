import { describe, it, expect } from 'vitest';
import { extractLabel, extractValue } from './extract-preview';

describe('extractValue', () => {
  it('is the value a filter takes', () => {
    expect(extractValue(['tok-1'], null)).toEqual({ kind: 'value', text: 'tok-1' });
    expect(extractValue([{ a: 1 }], null)).toEqual({ kind: 'value', text: '{"a":1}' });
  });

  it('says when nothing matched', () => {
    expect(extractValue([], null)).toEqual({ kind: 'none' });
    expect(extractLabel(extractValue([], null))).toBe('nothing matched — the run fails here');
  });

  it('says how many, when a filter yields several', () => {
    expect(extractValue([1, 2, 3], null)).toEqual({ kind: 'many', count: 3, text: '1' });
    expect(extractLabel(extractValue([1, 2, 3], null))).toBe('1 — and 2 more');
  });

  it('carries the reason a filter would not run', () => {
    expect(extractValue([], 'jq: syntax error')).toEqual({ kind: 'error', reason: 'jq: syntax error' });
  });
});

describe('a filter that matched null', () => {
  it('is not a value', () => {
    expect(extractValue([null], null)).toEqual({ kind: 'null' });
    expect(extractLabel(extractValue([null], null))).toBe('matched null — the next step sends null');
  });

  it('is still a value inside a list of results', () => {
    expect(extractValue([null, 1], null).kind).toBe('many');
  });
});
