import { describe, expect, it } from 'vitest';
import { emptyForm, joinPath, splitPath, splitForm, joinForm } from './query';

describe('the query a path carries', () => {
  it('is read as the parameters it holds', () => {
    expect(splitPath('/v1/users?page=2&sort=name')).toEqual({
      path: '/v1/users',
      params: [{ key: 'page', value: '2', bare: false }, { key: 'sort', value: 'name', bare: false }],
    });
  });

  it('is nothing when there is none', () => {
    expect(splitPath('/v1/users')).toEqual({ path: '/v1/users', params: [] });
    expect(splitPath('/v1/users?')).toEqual({ path: '/v1/users', params: [] });
  });

  it('keeps a parameter with no value, and one with no name', () => {
    expect(splitPath('/x?flag&=1')).toEqual({
      path: '/x',
      params: [{ key: 'flag', value: '', bare: true }, { key: '', value: '1', bare: false }],
    });
  });

  it('reads what was encoded', () => {
    expect(splitPath('/x?q=a%20b&plus=a+b').params).toEqual([
      { key: 'q', value: 'a b', bare: false },
      { key: 'plus', value: 'a b', bare: false },
    ]);
  });
});

describe('the path a request sends', () => {
  it('is the path when nothing is named', () => {
    expect(joinPath('/v1/users', [])).toBe('/v1/users');
    expect(joinPath('/v1/users', [{ key: '  ', value: 'x' }])).toBe('/v1/users');
  });

  it('carries the parameters in the order they were given', () => {
    expect(joinPath('/v1/users', [{ key: 'page', value: '2' }, { key: 'sort', value: 'name' }]))
      .toBe('/v1/users?page=2&sort=name');
  });

  it('encodes what would otherwise be a separator', () => {
    expect(joinPath('/x', [{ key: 'q', value: 'a b&c' }])).toBe('/x?q=a%20b%26c');
  });

  it('leaves a variable alone', () => {
    expect(joinPath('/x', [{ key: 'user', value: '{{USER}}' }])).toBe('/x?user={{USER}}');
  });

  it('writes a parameter with no value as a bare name', () => {
    expect(joinPath('/x', [{ key: 'flag', value: '' }])).toBe('/x?flag');
  });

  it('round-trips what it read', () => {
    const raw = '/v1/users?page=2&q=a%20b&flag';
    const { path, params } = splitPath(raw);
    expect(joinPath(path, params)).toBe(raw);
  });
});

describe('a form body', () => {
  it('is read as its pairs', () => {
    expect(splitForm('name=Ada&age=36')).toEqual([
      { key: 'name', value: 'Ada', bare: false },
      { key: 'age', value: '36', bare: false },
    ]);
    expect(splitForm('  ')).toEqual([]);
  });

  it('is written back as it was read', () => {
    expect(joinForm([{ key: 'name', value: 'Ada' }, { key: 'age', value: '36' }])).toBe('name=Ada&age=36');
    expect(joinForm([])).toBe('');
    expect(joinForm([{ key: 'q', value: 'a b' }])).toBe('q=a%20b');
  });

  it('round-trips', () => {
    const raw = 'name=Ada&flag&q=a%20b';
    expect(joinForm(splitForm(raw))).toBe(raw);
  });
});

describe('what the query editor must not rewrite', () => {
  it('keeps `flag=` as `flag=` and `flag` as `flag`', () => {
    expect(joinPath('/x', splitPath('/x?flag=').params)).toBe('/x?flag=');
    expect(joinPath('/x', splitPath('/x?flag').params)).toBe('/x?flag');
  });

  it('leaves an encoded brace encoded — it is text, not a variable', () => {
    const raw = '/x?user=%7B%7BUSER%7D%7D';
    const { path, params } = splitPath(raw);
    expect(params[0].value).toBe('%7B%7BUSER%7D%7D');
    expect(joinPath(path, params)).toBe(raw);
  });

  it('still reads a variable someone typed', () => {
    expect(joinPath('/x', splitPath('/x?user={{USER}}').params)).toBe('/x?user={{USER}}');
  });

  it('writes a row typed with no value yet as a bare name, as it always has', () => {
    expect(joinPath('/x', [{ key: 'flag', value: '' }])).toBe('/x?flag');
  });
});

describe('a parameter with no value', () => {
  it('is the name alone unless the file said otherwise', () => {
    expect(emptyForm({ key: 'flag', value: '' })).toBe('flag');
    expect(emptyForm({ key: 'flag', value: '', bare: true })).toBe('flag');
  });

  it('is `name=` when that is what it is', () => {
    expect(emptyForm({ key: 'flag', value: '', bare: false })).toBe('flag=');
  });

  it('and the path it writes says the same', () => {
    expect(joinPath('/v1/users', [{ key: 'flag', value: '' }])).toBe('/v1/users?flag');
    expect(joinPath('/v1/users', [{ key: 'flag', value: '', bare: false }])).toBe('/v1/users?flag=');
  });

  it('comes back as it was written', () => {
    expect(splitPath('/v1/users?flag').params).toEqual([{ key: 'flag', value: '', bare: true }]);
    expect(splitPath('/v1/users?flag=').params).toEqual([{ key: 'flag', value: '', bare: false }]);
  });
});
