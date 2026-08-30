import { describe, it, expect } from 'vitest';
import { hidesTyped, isSecretHeader, splitScheme, variableNameFor } from './secret-headers';

describe('isSecretHeader', () => {
  it('knows the keys whose values are credentials', () => {
    expect(isSecretHeader('authorization')).toBe(true);
    expect(isSecretHeader('Authorization')).toBe(true);
    expect(isSecretHeader('x-api-key')).toBe(true);
    expect(isSecretHeader('x-session-token')).toBe(true);
  });

  it('leaves an ordinary header alone', () => {
    expect(isSecretHeader('content-type')).toBe(false);
    expect(isSecretHeader('x-request-id')).toBe(false);
  });
});

describe('hidesTyped', () => {
  it('hides a credential typed inline', () => {
    expect(hidesTyped('authorization', 'Bearer abc123')).toBe(true);
  });

  it('shows a template, because a template is not a credential', () => {
    expect(hidesTyped('authorization', 'Bearer {{TOKEN}}')).toBe(false);
    expect(hidesTyped('x-api-key', '{{ KEY }}')).toBe(false);
  });

  it('never hides an ordinary header', () => {
    expect(hidesTyped('content-type', 'application/grpc')).toBe(false);
  });
});

describe('lifting a credential out of a header', () => {
  it('leaves the scheme where it is', () => {
    expect(splitScheme('Bearer sk-live-abc')).toEqual({ prefix: 'Bearer ', secret: 'sk-live-abc' });
    expect(splitScheme('basic  dXNlcjpwdw==')).toEqual({ prefix: 'basic ', secret: 'dXNlcjpwdw==' });
  });

  it('takes the whole value when there is no scheme', () => {
    expect(splitScheme('  sk-live-abc ')).toEqual({ prefix: '', secret: 'sk-live-abc' });
  });

  it('names the variable after the header', () => {
    expect(variableNameFor('authorization')).toBe('AUTH_TOKEN');
    expect(variableNameFor('x-api-key')).toBe('API_KEY');
    expect(variableNameFor('x-session-token')).toBe('SESSION_TOKEN');
    expect(variableNameFor('cookie')).toBe('COOKIE');
  });
});
