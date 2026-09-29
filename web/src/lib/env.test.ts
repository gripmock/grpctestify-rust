import { describe, it, expect } from 'vitest';
import { applyBindings, resolvedNames, unansweredNow, unusableNames, substituteEnv, findVariables, mergeEnvironments, mergeEnvLists } from './env';

const env = { name: 'dev', variables: { USER: 'ada', 'user.id': '7', EMPTY: '' } };

describe('placeholders follow the runner grammar', () => {
  it('substitutes with or without inner spaces', () => {
    expect(substituteEnv('{{USER}} and {{  USER  }}', env)).toBe('ada and ada');
  });

  it('accepts a dotted name', () => {
    expect(substituteEnv('id={{ user.id }}', env)).toBe('id=7');
  });

  it('leaves an unknown name literal, the way the runner does', () => {
    expect(substituteEnv('{{NOPE}}', env)).toBe('{{NOPE}}');
  });

  it('leaves anything that is not a name alone', () => {
    expect(substituteEnv('{{ a b }} {{1x}} {{}}', env)).toBe('{{ a b }} {{1x}} {{}}');
  });

  it('leaves an unclosed placeholder literal', () => {
    expect(substituteEnv('unclosed {{ USER', env)).toBe('unclosed {{ USER');
  });

  it('substitutes an empty value as empty', () => {
    expect(substituteEnv('[{{EMPTY}}]', env)).toBe('[]');
  });

  it('finds every distinct name once, spaces and dots included', () => {
    expect(findVariables('{{A}} {{ A }} {{ b.c }} {{ 1 }}')).toEqual(['A', 'b.c']);
  });
});

describe('merging environments', () => {
  it('drops a variable muted by an earlier environment', () => {
    const merged = mergeEnvironments([
      { name: 'base', variables: { K: '1' }, mutedVariables: ['K'] },
      { name: 'over', variables: { K: '2', J: '3' } },
    ]);
    expect(merged?.variables).toEqual({ J: '3' });
  });
});

describe('the two kinds of environment', () => {
  const env = (name: string, source: 'project' | 'browser') => ({ name, source, variables: {} }) as any;

  it('offers the project first and keeps the browser one out of its way', () => {
    const { list, shadowed } = mergeEnvLists(
      [env('staging', 'project')],
      [env('staging', 'browser'), env('mine', 'browser')],
    );
    expect(list.map(e => `${e.name}:${e.source}`)).toEqual(['staging:project', 'mine:browser']);
    expect(shadowed).toEqual(['staging']);
  });

  it('is just the browser list when there is no project', () => {
    const { list, shadowed } = mergeEnvLists([], [env('mine', 'browser')]);
    expect(list.map(e => e.name)).toEqual(['mine']);
    expect(shadowed).toEqual([]);
  });
});

describe('applyBindings', () => {
  it('substitutes what a run bound, in the endpoint, the headers and the body', () => {
    const out = applyBindings(
      'GET /v1/orders/{{id}}',
      { authorization: 'Bearer {{token}}' },
      ['{"who": "{{who}}"}'],
      [['id', '7'], ['token', 'abc'], ['who', 'ok']],
    );
    expect(out.endpoint).toBe('GET /v1/orders/7');
    expect(out.headers.authorization).toBe('Bearer abc');
    expect(out.bodies[0]).toBe('{"who": "ok"}');
  });

  it('leaves a name no run bound as it is written', () => {
    const out = applyBindings('{{a}} {{b}}', {}, [], [['a', '1']]);
    expect(out.endpoint).toBe('1 {{b}}');
  });

  it('changes nothing when no run has bound anything', () => {
    const out = applyBindings('{{a}}', { h: '{{a}}' }, ['{{a}}'], undefined);
    expect([out.endpoint, out.headers.h, out.bodies[0]]).toEqual(['{{a}}', '{{a}}', '{{a}}']);
  });
});

describe('resolvedNames', () => {
  const env = { name: 'e', variables: { token: 'abc', blank: '' }, address: '', mutedVariables: ['blank'] };

  it('names what the environment and the run answered for', () => {
    expect(resolvedNames(['{{token}} {{who}}'], [['who', 'ok']], env).sort()).toEqual(['token', 'who']);
  });

  it('says nothing about a name nothing answers for', () => {
    expect(resolvedNames(['{{nobody}}'], undefined, env)).toEqual([]);
  });

  it('leaves a muted name out', () => {
    expect(resolvedNames(['{{blank}}'], undefined, env)).toEqual([]);
  });

  it('reads every part of the request once', () => {
    expect(resolvedNames(['{{token}}', 'x {{token}}', '{{who}}'], [['who', '1']], env).sort())
      .toEqual(['token', 'who']);
  });
});

describe('unansweredNow', () => {
  it('names what was resolved then and is not now', () => {
    expect(unansweredNow(['who', 'token'], ['token'])).toEqual(['who']);
  });

  it('says nothing when everything is still answered', () => {
    expect(unansweredNow(['who'], ['who', 'token'])).toEqual([]);
  });

  it('says nothing about a line that recorded none', () => {
    expect(unansweredNow(undefined, [])).toEqual([]);
    expect(unansweredNow([], ['who'])).toEqual([]);
  });
});

describe('unusableNames', () => {
  it('names what substitution will not read', () => {
    expect(unusableNames(['TOKEN', 'a-b', 'has space', '1st'])).toEqual(['a-b', 'has space', '1st']);
  });

  it('accepts the grammar a file uses', () => {
    expect(unusableNames(['TOKEN', '_x', 'a.b', 'A1_2'])).toEqual([]);
  });

  it('says nothing about an empty row', () => {
    expect(unusableNames(['', '   '])).toEqual([]);
  });
});
