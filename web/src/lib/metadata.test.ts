import { describe, it, expect } from 'vitest';
import { checkMetadataKey, checkMetadataValue, isBase64 } from './metadata';

describe('checkMetadataKey', () => {
  it('passes an ordinary key', () => {
    expect(checkMetadataKey('x-api-key')).toBeNull();
    expect(checkMetadataKey('trace.id_1')).toBeNull();
  });

  it('refuses what the wire refuses', () => {
    expect(checkMetadataKey('')?.level).toBe('bad');
    expect(checkMetadataKey(':path')?.level).toBe('bad');
    expect(checkMetadataKey('x api key')?.level).toBe('bad');
    expect(checkMetadataKey('x@key')?.level).toBe('bad');
  });

  it('says what happens to an uppercase key rather than refusing it', () => {
    const note = checkMetadataKey('Authorization');
    expect(note?.level).toBe('note');
    expect(note?.reason).toContain('authorization');
  });
});

describe('checkMetadataValue', () => {
  it('leaves an empty value alone — the key is the problem, if any', () => {
    expect(checkMetadataValue('x-bin', '')).toBeNull();
  });

  it('wants base64 behind a -bin key', () => {
    expect(checkMetadataValue('trace-bin', 'AAEC')).toBeNull();
    expect(checkMetadataValue('trace-bin', 'AAE')).toBeNull();
    expect(checkMetadataValue('TRACE-BIN', 'AAEC')).toBeNull();
    expect(checkMetadataValue('trace-bin', 'not base64!')?.level).toBe('bad');
  });

  it('keeps non-ASCII out of a text header', () => {
    expect(checkMetadataValue('x-note', 'ok')).toBeNull();
    expect(checkMetadataValue('x-note', 'привет')?.level).toBe('bad');
  });
});

describe('isBase64', () => {
  it('accepts padded and unpadded, refuses the impossible remainder', () => {
    expect(isBase64('AAEC')).toBe(true);
    expect(isBase64('QQ==')).toBe(true);
    expect(isBase64('QQ')).toBe(true);
    expect(isBase64('A')).toBe(false);
  });
});

describe('a header on the HTTP wire', () => {
  it('says nothing about a capital letter', () => {
    expect(checkMetadataKey('Content-Type', 'http')).toBeNull();
    expect(checkMetadataKey('Content-Type')?.reason).toContain('lowercases');
  });

  it('still refuses a name that cannot be sent', () => {
    expect(checkMetadataKey('', 'http')?.level).toBe('bad');
    expect(checkMetadataKey(':status', 'http')?.level).toBe('bad');
    expect(checkMetadataKey('a b', 'http')?.level).toBe('bad');
  });

  it('has no -bin convention: that is gRPC binary metadata', () => {
    expect(checkMetadataValue('x-token-bin', 'not base64!!', 'http')).toBeNull();
    expect(checkMetadataValue('x-token-bin', 'not base64!!')?.level).toBe('bad');
  });

  it('refuses a value that is not printable ASCII, as both wires do', () => {
    expect(checkMetadataValue('x-name', 'Ada 😀', 'http')?.level).toBe('bad');
    expect(checkMetadataValue('x-name', 'Ada', 'http')).toBeNull();
  });
});

describe('a value that is still a template', () => {
  it('is not judged as the value it stands for', () => {
    expect(checkMetadataValue('trace-bin', '{{TRACE}}')).toBeNull();
    expect(checkMetadataValue('authorization', 'Bearer {{TOKEN}}')).toBeNull();
  });

  it('still judges a value that holds no variable', () => {
    expect(checkMetadataValue('trace-bin', 'not base64!')?.level).toBe('bad');
  });
});

describe('a length written by hand', () => {
  it('is named and dropped, on HTTP', () => {
    expect(checkMetadataKey('content-length', 'http')?.level).toBe('bad');
    expect(checkMetadataKey('Content-Length', 'http')?.reason).toContain('dropped');
  });

  it('says nothing about other headers', () => {
    expect(checkMetadataKey('content-type', 'http')).toBe(null);
    expect(checkMetadataKey('host', 'http')).toBe(null);
  });
});
