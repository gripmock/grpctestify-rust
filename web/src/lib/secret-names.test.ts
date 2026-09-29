import { describe, it, expect } from 'vitest';
import { looksLikeSecret, maskValue } from './secret-names';

describe('a variable whose name says it holds a credential', () => {
  it('is the ones an environment usually calls them', () => {
    for (const name of ['TOKEN', 'API_KEY', 'apikey', 'DB_PASSWORD', 'auth_header', 'PRIVATE_KEY', 'client_secret']) {
      expect(looksLikeSecret(name)).toBe(true);
    }
  });

  it('is not everything else', () => {
    for (const name of ['USER', 'HOST', 'BASE_URL', 'REGION', '']) {
      expect(looksLikeSecret(name)).toBe(false);
    }
  });
});

describe('a value on its way into a tooltip', () => {
  it('is covered when its name says it is a credential', () => {
    expect(maskValue('TOKEN', 'ey.real')).toBe('••••••');
    expect(maskValue('USER', 'Ada')).toBe('Ada');
  });

  it('is nothing when there is nothing', () => {
    expect(maskValue('TOKEN', '')).toBe('');
    expect(maskValue('TOKEN', undefined)).toBe('');
  });
});

describe('a name the workbench was told is a credential', () => {
  it('is hidden even when nothing about the word says so', () => {
    expect(looksLikeSecret('SEED', ['SEED'])).toBe(true);
    expect(maskValue('SEED', 'abc', ['SEED'])).toBe('••••••');
  });

  it('is matched however either side spells the case', () => {
    expect(maskValue('seed', 'abc', ['SEED'])).toBe('••••••');
    expect(maskValue('SEED', 'abc', [' seed '])).toBe('••••••');
  });

  it('leaves the names it was not told about to the words in them', () => {
    expect(maskValue('HOST', 'api.test', ['SEED'])).toBe('api.test');
    expect(maskValue('API_TOKEN', 'abc', ['SEED'])).toBe('••••••');
  });
});
