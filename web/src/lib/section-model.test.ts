import { describe, it, expect } from 'vitest';
import {
  setKey, numberValue, isTruthy, tlsModeOf, applyTlsMode,
  protoSourceOf, applyProtoSource, csvList, csvJoin,
} from './section-model';

describe('setKey', () => {
  it('removes the key when the field is cleared, rather than writing an empty value', () => {
    expect(setKey({ timeout: '5' }, 'timeout', '')).toEqual({});
  });
  it('sets and overwrites', () => {
    expect(setKey({}, 'retry', '3')).toEqual({ retry: '3' });
    expect(setKey({ retry: '3' }, 'retry', '4')).toEqual({ retry: '4' });
  });
});

describe('numberValue', () => {
  it('accepts what the validator accepts', () => {
    expect(numberValue('5', { integer: true, min: 1 })).toBe('5');
    expect(numberValue('1.5', { min: 0 })).toBe('1.5');
    expect(numberValue('0', { min: 0 })).toBe('0');
  });
  it('rejects what the validator would flag', () => {
    expect(numberValue('0', { integer: true, min: 1 })).toBeNull();
    expect(numberValue('1.5', { integer: true })).toBeNull();
    expect(numberValue('-1', { min: 0 })).toBeNull();
    expect(numberValue('abc')).toBeNull();
  });
  it('treats blank as "unset", not invalid', () => {
    expect(numberValue('')).toBe('');
  });
});

describe('isTruthy', () => {
  it('matches the parser\'s boolean vocabulary', () => {
    for (const v of ['true', '1', 'yes', 'on', 'TRUE']) expect(isTruthy(v)).toBe(true);
    for (const v of ['false', '0', 'no', 'off', '', undefined]) expect(isTruthy(v)).toBe(false);
  });
});

describe('TLS as one axis', () => {
  it('reads the three states', () => {
    expect(tlsModeOf({})).toBe('plaintext');
    expect(tlsModeOf({ ca_cert: '/ca.pem' })).toBe('tls');
    expect(tlsModeOf({ insecure: 'true' })).toBe('insecure');
  });

  it('plaintext drops the whole section', () => {
    expect(applyTlsMode({ ca_cert: '/ca.pem', insecure: 'true' }, 'plaintext')).toEqual({});
  });

  it('switching to verified TLS removes the insecure flag but keeps the paths', () => {
    expect(applyTlsMode({ ca_cert: '/ca.pem', insecure: 'true' }, 'tls')).toEqual({ ca_cert: '/ca.pem' });
  });

  it('switching to insecure sets the flag', () => {
    expect(applyTlsMode({ ca_cert: '/ca.pem' }, 'insecure')).toEqual({ ca_cert: '/ca.pem', insecure: 'true' });
  });
});

describe('PROTO source as a strategy', () => {
  it('reads which one the file uses', () => {
    expect(protoSourceOf({})).toBe('reflection');
    expect(protoSourceOf({ descriptor: 'a.desc' })).toBe('descriptor');
    expect(protoSourceOf({ files: 'a.proto' })).toBe('files');
  });

  it('switching drops the keys the other strategy owns', () => {
    expect(applyProtoSource({ files: 'a.proto', import_paths: '/p' }, 'descriptor')).toEqual({});
    expect(applyProtoSource({ descriptor: 'a.desc' }, 'files')).toEqual({});
    expect(applyProtoSource({ descriptor: 'a.desc' }, 'reflection')).toEqual({});
  });
});

describe('comma-separated lists', () => {
  it('round-trips, trimming and dropping blanks', () => {
    expect(csvList(' a.proto , b.proto ,')).toEqual(['a.proto', 'b.proto']);
    expect(csvJoin(['a.proto', '', 'b.proto'])).toBe('a.proto,b.proto');
    expect(csvList(undefined)).toEqual([]);
  });
});
