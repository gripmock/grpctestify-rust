import { describe, it, expect } from 'vitest';
import { TLS_ALIASES, aliasValue, setAlias, unknownKeys } from './section-model';

describe('a field the file may spell several ways', () => {
  it('reads whichever spelling is there', () => {
    expect(aliasValue({ cert_file: '/x.pem' }, TLS_ALIASES.client_cert)).toBe('/x.pem');
    expect(aliasValue({ client_cert: '/y.pem' }, TLS_ALIASES.client_cert)).toBe('/y.pem');
    expect(aliasValue({}, TLS_ALIASES.client_cert)).toBe('');
  });

  it('writes back to the spelling already in the file', () => {
    expect(setAlias({ cert_file: '/x.pem' }, TLS_ALIASES.client_cert, '/z.pem')).toEqual({ cert_file: '/z.pem' });
  });

  it('uses the canonical spelling for a field the file does not have', () => {
    expect(setAlias({}, TLS_ALIASES.client_cert, '/z.pem')).toEqual({ client_cert: '/z.pem' });
  });

  it('removes the key it read when the field is cleared', () => {
    expect(setAlias({ cert_file: '/x.pem', insecure: 'true' }, TLS_ALIASES.client_cert, '')).toEqual({ insecure: 'true' });
  });
});

describe('unknownKeys', () => {
  it('names what the form does not speak for', () => {
    expect(unknownKeys({ insecure: 'true', mystery: '1' }, ['insecure'])).toEqual([['mystery', '1']]);
  });
});
