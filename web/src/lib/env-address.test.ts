import { describe, expect, it } from 'vitest';
import { aimsHttp, environmentAddressNote, targetNote } from './env-address';

describe('an environment target', () => {
  it('aims both families when it carries a scheme', () => {
    expect(aimsHttp('http://127.0.0.1:8899')).toBe(true);
    expect(aimsHttp('https://api.acme.io')).toBe(true);
    expect(targetNote('http://127.0.0.1:8899')).toBeNull();
  });

  it('aims only the gRPC files without one', () => {
    expect(aimsHttp('127.0.0.1:4770')).toBe(false);
    expect(targetNote('127.0.0.1:4770')).toContain('needs a scheme');
  });

  it('says nothing about a target nobody has typed', () => {
    expect(targetNote('')).toBeNull();
    expect(targetNote('   ')).toBeNull();
  });
});

describe('what to say about an environment address', () => {
  it('says what is wrong with one nothing can dial', () => {
    expect(environmentAddressNote('localhost:99999')).toEqual({
      said: 'A port is between 1 and 65535',
      bad: true,
    });
    expect(environmentAddressNote('not a host')?.bad).toBe(true);
  });

  it('says nothing about the two shapes that work', () => {
    expect(environmentAddressNote('localhost:4770')).toBeNull();
    expect(environmentAddressNote('https://api.test')).toBeNull();
  });

  it('leaves a bare host to the note that already speaks for it', () => {
    expect(environmentAddressNote('api.test')).toBeNull();
  });
});
