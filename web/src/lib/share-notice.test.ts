import { describe, it, expect } from 'vitest';
import { bodyWarnings, credentialLooking } from './share-notice';

describe('credentialLooking', () => {
  it('knows the shapes a credential usually takes', () => {
    expect(credentialLooking('{"token":"eyJhbGciOiJIUzI1NiJ9.abc.def"}')).toBe('looks like a JWT');
    expect(credentialLooking('Bearer sk-abcdefghijkl')).toBe('looks like a bearer token');
    expect(credentialLooking('{"password": "hunter2"}')).toBe('has a field named like a credential');
    expect(credentialLooking('-----BEGIN RSA PRIVATE KEY-----')).toBe('carries a private key');
  });

  it('leaves ordinary payloads alone', () => {
    expect(credentialLooking('{"email":"a@b.io","count":3}')).toBeNull();
    expect(credentialLooking('{"tokens_used": 12}')).toBeNull();
    expect(credentialLooking('')).toBeNull();
  });
});

describe('bodyWarnings', () => {
  it('names the message the warning is about', () => {
    expect(bodyWarnings(['{}', '{"api_key": "x"}'])).toEqual([
      { index: 1, reason: 'has a field named like a credential' },
    ]);
  });

  it('says nothing when there is nothing to say', () => {
    expect(bodyWarnings(['{}', '{"a":1}'])).toEqual([]);
  });
});
