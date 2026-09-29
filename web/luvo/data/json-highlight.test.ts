import { describe, it, expect } from 'vitest';
import { tokenizeJson } from 'luvo/data/json-highlight';

const kinds = (text: string) => tokenizeJson(text).filter(t => t.text.trim() !== '').map(t => `${t.kind}:${t.text}`);

describe('JSON, coloured the way the response pane colours it', () => {
  it('tells a key from the string it holds', () => {
    expect(kinds('{"name": "World"}')).toEqual([
      'punct:{', 'key:"name"', 'punct::', 'str:"World"', 'punct:}',
    ]);
  });

  it('reads numbers, booleans and null as values', () => {
    expect(kinds('{"n": 3.5, "ok": true, "x": null}')).toContain('num:3.5');
    expect(kinds('{"ok": true}')).toContain('num:true');
    expect(kinds('{"x": null}')).toContain('num:null');
  });

  it('leaves an escaped quote inside a string alone', () => {
    expect(kinds('{"s": "a\\"b"}')).toContain('str:"a\\"b"');
  });

  it('passes text that is not JSON through', () => {
    expect(tokenizeJson('hello').map(t => t.kind)).toEqual(['plain']);
  });
});
