import { describe, expect, it } from 'vitest';
import { browserSeed, newSeed, openingSeed } from './env-seed';
import type { Environment } from './types';

const dev: Environment = { name: 'dev', source: 'browser', variables: { HOST: 'h' }, tls: true };

describe('what the manager opens on', () => {
  it('starts a new environment around the name that was asked for', () => {
    const seed = newSeed('API_KEY', 'k');
    expect(seed.view).toEqual({ kind: 'new' });
    expect(seed.rows[0]).toEqual({ key: 'API_KEY', value: 'k', local: true });
    expect(seed.rows).toHaveLength(2);
  });

  it('adds the asked-for name to a browser environment only when it is missing', () => {
    expect(browserSeed(dev, 'TOKEN').rows.map(r => r.key)).toEqual(['HOST', 'TOKEN', '']);
    expect(browserSeed(dev, 'HOST').rows.map(r => r.key)).toEqual(['HOST', '']);
  });

  it('verifies certificates unless the environment says not to', () => {
    expect(browserSeed(dev).tlsInsecure).toBe(false);
    expect(browserSeed({ ...dev, tlsInsecure: true }).tlsInsecure).toBe(true);
    expect(newSeed('X').tlsInsecure).toBe(false);
  });

  it('opens nothing without a name to define, and leaves a project file to be read', () => {
    expect(openingSeed(null, undefined, dev)).toBeNull();
    expect(openingSeed('X', undefined, undefined)?.view).toEqual({ kind: 'new' });
    expect(openingSeed('X', undefined, dev)?.view).toEqual({ kind: 'edit', name: 'dev', origin: 'browser' });
    expect(openingSeed('X', undefined, { ...dev, source: 'project' })).toBeNull();
  });
});
