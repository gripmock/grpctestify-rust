import { describe, it, expect } from 'vitest';
import { bytesToBase64, protoKindOf, refusalFor } from './proto-files';

describe('protoKindOf', () => {
  it('tells source from a compiled set, whatever the repo calls it', () => {
    expect(protoKindOf('auth.proto')).toBe('proto');
    expect(protoKindOf('schema.pb')).toBe('descriptor');
    expect(protoKindOf('schema.bin')).toBe('descriptor');
    expect(protoKindOf('schema.desc')).toBe('descriptor');
    expect(protoKindOf('SCHEMA.PROTOSET')).toBe('descriptor');
  });

  it('refuses anything else, including a bare name', () => {
    expect(protoKindOf('notes.txt')).toBeNull();
    expect(protoKindOf('proto')).toBeNull();
    expect(protoKindOf('')).toBeNull();
  });
});

describe('bytesToBase64', () => {
  it('encodes bytes text encoding would mangle', () => {
    expect(bytesToBase64(new Uint8Array([0x0a, 0x00, 0xff, 0x7f, 0x41]))).toBe('CgD/f0E=');
  });

  it('handles an empty file and one longer than a chunk', () => {
    expect(bytesToBase64(new Uint8Array([]))).toBe('');
    const big = new Uint8Array(0x8000 + 5).fill(0x41);
    expect(atob(bytesToBase64(big)).length).toBe(big.length);
  });
});

describe('refusalFor', () => {
  it('names every kind that is accepted', () => {
    const said = refusalFor('notes.txt');
    expect(said).toContain('notes.txt');
    expect(said).toContain('.proto');
    expect(said).toContain('.pb');
  });
});
