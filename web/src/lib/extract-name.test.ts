import { describe, expect, it } from 'vitest';
import { splitExtractName, writtenExtractName } from './extract-name';

describe('a binding’s name and the type it carries', () => {
  it('reads the type off the name', () => {
    expect(splitExtractName('price:number')).toEqual({ name: 'price', kind: 'number' });
    expect(splitExtractName(' created:time ')).toEqual({ name: 'created', kind: 'time' });
  });

  it('is a plain name when it carries none', () => {
    expect(splitExtractName('user_id')).toEqual({ name: 'user_id', kind: null });
    expect(splitExtractName('a.b')).toEqual({ name: 'a.b', kind: null });
  });

  it('leaves anything that is not a type alone', () => {
    expect(splitExtractName('path:')).toEqual({ name: 'path:', kind: null });
    expect(splitExtractName('weird:a b')).toEqual({ name: 'weird:a b', kind: null });
    expect(splitExtractName('a:b:c')).toEqual({ name: 'a:b', kind: 'c' });
  });

  it('writes it back the way the file writes it', () => {
    expect(writtenExtractName('price', 'number')).toBe('price:number');
    expect(writtenExtractName('price', null)).toBe('price');
    expect(writtenExtractName('price', undefined)).toBe('price');
  });
});
