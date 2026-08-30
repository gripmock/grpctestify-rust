import { describe, expect, it } from 'vitest';
import { caUnused, halfIdentity } from './tls-shape';

describe('a CA nothing reads', () => {
  it('is one named under insecure', () => {
    expect(caUnused({ insecure: 'true', ca_cert: '/etc/ca.pem' })).toBe(true);
    expect(caUnused({ insecure: 'true', ca_file: '/etc/ca.pem' })).toBe(true);
  });

  it('is read when the file verifies, and is nothing when none is named', () => {
    expect(caUnused({ ca_cert: '/etc/ca.pem' })).toBe(false);
    expect(caUnused({ insecure: 'false', ca_cert: '/etc/ca.pem' })).toBe(false);
    expect(caUnused({ insecure: 'true' })).toBe(false);
  });
});

describe('half a client identity', () => {
  it('names the half that is missing', () => {
    expect(halfIdentity({ client_cert: '/c.pem' })).toBe('client_key');
    expect(halfIdentity({ key_file: '/c.key' })).toBe('client_cert');
  });

  it('says nothing about a pair, or about neither', () => {
    expect(halfIdentity({ client_cert: '/c.pem', client_key: '/c.key' })).toBeNull();
    expect(halfIdentity({ cert_file: '/c.pem', key_file: '/c.key' })).toBeNull();
    expect(halfIdentity({ ca_cert: '/ca.pem' })).toBeNull();
  });

  it('reads a blank value as absent', () => {
    expect(halfIdentity({ client_cert: '/c.pem', client_key: '  ' })).toBe('client_key');
  });
});
