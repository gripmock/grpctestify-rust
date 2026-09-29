import { describe, it, expect } from 'vitest';
import { apiPath } from './api-path';

describe('a collection path in a URL', () => {
  it('keeps the separators and encodes the rest', () => {
    expect(apiPath('auth/login.gctf')).toBe('auth/login.gctf');
    expect(apiPath('with space.httf')).toBe('with%20space.httf');
  });

  it('encodes what a URL would otherwise read as syntax', () => {
    expect(apiPath('hash#tag.httf')).toBe('hash%23tag.httf');
    expect(apiPath('q?x.httf')).toBe('q%3Fx.httf');
    expect(apiPath('100%.httf')).toBe('100%25.httf');
  });

  it('leaves an empty path alone', () => {
    expect(apiPath('')).toBe('');
  });
});
