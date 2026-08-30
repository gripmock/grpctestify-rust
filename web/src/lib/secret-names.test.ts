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
