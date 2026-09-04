import { describe, it, expect } from 'vitest';
import { envUsage, unresolvedCount } from './env-usage';

const env = { name: 'dev', variables: { USER: 'ada', SECRET: '' }, mutedVariables: ['SECRET'] };

describe('what a body asks of the environment', () => {
  it('marks a set variable resolved and keeps its value for the hover', () => {
    expect(envUsage('{{USER}}', env)).toEqual([
      { key: 'USER', value: 'ada', muted: false, resolved: true, from: 'env' },
    ]);
  });

  it('counts a muted variable as unresolved — the runner leaves it as written', () => {
    const uses = envUsage('{{SECRET}}', env);
    expect(uses[0].muted).toBe(true);
    expect(uses[0].resolved).toBe(false);
    expect(unresolvedCount(uses)).toBe(1);
  });

  it('lists a variable no environment answers for', () => {
    expect(envUsage('{{ NOPE }}', env)).toEqual([
      { key: 'NOPE', value: undefined, muted: false, resolved: false, from: 'unknown' },
    ]);
  });

  it('treats every variable as unresolved when no environment is active', () => {
    expect(unresolvedCount(envUsage('{{A}} {{B}}', null))).toBe(2);
  });

  it('ignores braces that are not a variable name', () => {
    expect(envUsage('{{ not a name }} {{}}', env)).toEqual([]);
  });
});

describe('what answers for a variable', () => {
  const env = { name: 'dev', variables: { HOST: 'h' }, mutedVariables: [] } as any;

  it('is the environment when it has it', () => {
    expect(envUsage('{{HOST}}', env)[0]).toMatchObject({ from: 'env', resolved: true });
  });

  it('is a dataset column for {{dataset.x}}', () => {
    expect(envUsage('{{dataset.email}}', env, { datasetColumns: ['email'] })[0])
      .toMatchObject({ from: 'dataset', resolved: true });
  });

  it('is an earlier step for a name it extracts', () => {
    expect(envUsage('{{token}}', env, { extracted: ['token'] })[0])
      .toMatchObject({ from: 'extract', resolved: true });
  });

  it('is nobody when no one has it', () => {
    expect(envUsage('{{dataset.missing}}', env, { datasetColumns: ['email'] })[0])
      .toMatchObject({ from: 'unknown', resolved: false });
    expect(envUsage('{{nope}}', env)[0]).toMatchObject({ from: 'unknown', resolved: false });
  });

  it('counts only the ones nobody answers for', () => {
    const uses = envUsage('{{HOST}} {{dataset.email}} {{nope}}', env, { datasetColumns: ['email'] });
    expect(unresolvedCount(uses)).toBe(1);
  });
});

describe('a name the environment answers with nothing', () => {
  it('is resolved, and says it is empty', () => {
    const env = { name: 'e', source: 'project' as const, variables: { TOKEN: '', USER: 'Ada' } };
    const [token, user] = envUsage('{{TOKEN}} {{USER}}', env);
    expect(token).toMatchObject({ resolved: true, empty: true });
    expect(user.empty).toBeUndefined();
  });

  it('is not counted as unresolved — it has an answer', () => {
    const env = { name: 'e', source: 'project' as const, variables: { TOKEN: '' } };
    expect(unresolvedCount(envUsage('{{TOKEN}}', env))).toBe(0);
  });
});

describe('a name the project environment answers for', () => {
  it('is resolved where the call is made, not sent as written', () => {
    const uses = envUsage('GET /v1/{{WHO}}', null, { projectNames: ['WHO'] });
    expect(uses).toEqual([{ key: 'WHO', value: undefined, muted: false, resolved: true, from: 'project' }]);
    expect(unresolvedCount(uses)).toBe(0);
  });

  it('is still unresolved when the project does not hold it', () => {
    expect(unresolvedCount(envUsage('GET /v1/{{WHO}}', null, { projectNames: ['OTHER'] }))).toBe(1);
  });

  it('yields to the environment the browser can see', () => {
    const env = { name: 'local', variables: { WHO: 'Ada' } };
    expect(envUsage('{{WHO}}', env, { projectNames: ['WHO'] })[0].from).toBe('env');
  });
});

describe('what only a run can answer', () => {
  const runtime = { datasetColumns: ['email'], extracted: ['token'], projectNames: ['HOST'] };

  it('counts a row and an earlier step as answered for a run', () => {
    const uses = envUsage('{{dataset.email}} {{token}} {{HOST}}', null, { ...runtime, mode: 'run' });
    expect(uses.map(u => u.resolved)).toEqual([true, true, true]);
  });

  it('counts them unanswered for Execute, and says who would have', () => {
    const uses = envUsage('{{dataset.email}} {{token}} {{HOST}}', null, { ...runtime, mode: 'execute' });
    expect(uses.map(u => ({ key: u.key, resolved: u.resolved, from: u.from, runOnly: u.runOnly }))).toEqual([
      { key: 'dataset.email', resolved: false, from: 'dataset', runOnly: true },
      { key: 'token', resolved: false, from: 'extract', runOnly: true },
      { key: 'HOST', resolved: true, from: 'project', runOnly: undefined },
    ]);
  });
});

describe('the columns a source answers', () => {
  it('are answered for a run and not for Execute', () => {
    const runtime = { sourceColumns: ['paths.file', 'paths.host'] };
    const [run] = envUsage('{{paths.file}}', null, { ...runtime, mode: 'run' });
    expect({ resolved: run.resolved, from: run.from }).toEqual({ resolved: true, from: 'source' });

    const [execute] = envUsage('{{paths.file}}', null, { ...runtime, mode: 'execute' });
    expect({ resolved: execute.resolved, from: execute.from, runOnly: execute.runOnly })
      .toEqual({ resolved: false, from: 'source', runOnly: true });
  });

  it('are only the columns it has', () => {
    const [use] = envUsage('{{paths.missing}}', null, { sourceColumns: ['paths.file'], mode: 'run' });
    expect({ resolved: use.resolved, from: use.from }).toEqual({ resolved: false, from: 'unknown' });
  });
});

describe('what a run of this file already bound', () => {
  it('is a value, not a promise, so Execute resolves it', () => {
    const [use] = envUsage('{{who}}', null, { runBound: [['who', 'ok']], mode: 'execute' });
    expect(use).toMatchObject({ key: 'who', value: 'ok', resolved: true, from: 'run' });
    expect(use.runOnly).toBeUndefined();
  });

  it('wins over the environment', () => {
    const env = { name: 'e', variables: { who: 'from env' }, address: '' };
    const [use] = envUsage('{{who}}', env, { runBound: [['who', 'from the run']] });
    expect(use).toMatchObject({ value: 'from the run', from: 'run' });
  });

  it('says nothing about a name no run bound', () => {
    const [use] = envUsage('{{other}}', null, { runBound: [['who', 'ok']], extracted: ['other'], mode: 'execute' });
    expect(use).toMatchObject({ key: 'other', resolved: false, from: 'extract', runOnly: true });
  });
});

describe('a dataset column with a row picked', () => {
  it('resolves against the row', () => {
    const used = envUsage('{"name": "{{dataset.who}}"}', null, {
      datasetColumns: ['who'],
      datasetRowValues: { who: 'World' },
      mode: 'execute',
    });
    expect(used[0].resolved).toBe(true);
    expect(used[0].value).toBe('World');
    expect(used[0].from).toBe('dataset');
  });

  it('is still a promise a run keeps when no row is picked', () => {
    const used = envUsage('{"name": "{{dataset.who}}"}', null, {
      datasetColumns: ['who'],
      datasetRowValues: null,
      mode: 'execute',
    });
    expect(used[0].resolved).toBe(false);
    expect(used[0].runOnly).toBe(true);
  });

  it('marks an empty cell answered and empty', () => {
    const used = envUsage('{{dataset.who}}', null, {
      datasetColumns: ['who'],
      datasetRowValues: { who: '' },
      mode: 'execute',
    });
    expect(used[0].resolved).toBe(true);
    expect(used[0].empty).toBe(true);
  });
});
