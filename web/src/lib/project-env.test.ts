import { describe, expect, it } from 'vitest';
import { projectEnvFile, projectEnvLocal } from './project-env';

describe('what the workbench says an environment file holds', () => {
  it('reads the text out of the object, never the object itself', () => {
    const said = projectEnvFile({ content: 'HOST=api.test\n', secret: ['SEED'] });
    expect(said.content).toBe('HOST=api.test\n');
    expect(said.secret).toEqual(['SEED']);
  });

  it('has no text when the answer is not the shape it should be', () => {
    expect(projectEnvFile({ secret: ['SEED'] })).toEqual({ content: '', secret: ['SEED'] });
    expect(projectEnvFile(null)).toEqual({ content: '', secret: [] });
    expect(projectEnvFile({ content: 42 })).toEqual({ content: '', secret: [] });
  });

  it('still reads a bare string, which is what the file used to come back as', () => {
    expect(projectEnvFile('HOST=api.test\n')).toEqual({ content: 'HOST=api.test\n', secret: [] });
  });

  it('keeps only the names, out of whatever the list holds', () => {
    expect(projectEnvFile({ content: '', secret: ['SEED', 7, null] }).secret).toEqual(['SEED']);
    expect(projectEnvFile({ content: '', secret: 'SEED' }).secret).toEqual([]);
  });
});

describe('what the workbench says about the local half', () => {
  it('reads the file, and the names it says are credentials', () => {
    expect(projectEnvLocal({ exists: true, content: 'SEED=abc\n', secret: ['SEED'] }))
      .toEqual({ exists: true, content: 'SEED=abc\n', secret: ['SEED'] });
  });

  it('names nothing when there is no local file', () => {
    expect(projectEnvLocal({ exists: false, content: null, secret: [] }))
      .toEqual({ exists: false, content: null, secret: [] });
    expect(projectEnvLocal(null)).toEqual({ exists: false, content: null, secret: [] });
  });
});
