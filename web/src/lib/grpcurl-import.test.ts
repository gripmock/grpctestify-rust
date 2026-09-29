import { describe, it, expect } from 'vitest';
import { importSummary, planImport, type ImportedCommand } from './grpcurl-import';

const base: ImportedCommand = {
  endpoint: 'svc.G/S', address: 'h:4770', headers: {}, body: '{}', plaintext: true,
};

describe('planImport', () => {
  it('maps the flags a section can hold', () => {
    const plan = planImport({ ...base, options: { 'max-time': '12', compression: 'gzip', plaintext: 'true' } });
    expect(plan.options).toEqual({ timeout: '12', compression: 'gzip' });
    expect(plan.ignored).toEqual([]);
  });

  it('names what it cannot honour', () => {
    const plan = planImport({ ...base, options: { 'emit-defaults': 'true', format: 'text' } });
    expect(plan.options).toEqual({});
    expect(plan.ignored).toEqual(['-emit-defaults', '-format text']);
  });
});

describe('importSummary', () => {
  it('says which sections it filled', () => {
    const imported = { ...base, headers: { a: '1' }, tls: { ca_cert: '/x' }, proto: { files: 'a.proto' } };
    expect(importSummary(imported, planImport({ ...imported, options: { 'max-time': '5' } })))
      .toBe('with 1 header, TLS, PROTO, OPTIONS');
  });

  it('says nothing when there was nothing besides the call', () => {
    expect(importSummary(base, planImport(base))).toBe('');
  });
});

describe('a max-time grpcurl took as a fraction', () => {
  it('rounds up and says so', () => {
    const plan = planImport({ endpoint: 'a.B/C', address: 'x:1', headers: {}, body: '{}', plaintext: true, options: { 'max-time': '2.5' } });
    expect(plan.options.timeout).toBe('3');
    expect(plan.adjusted.join(' ')).toContain('whole seconds');
  });

  it('leaves a whole number alone', () => {
    const plan = planImport({ endpoint: 'a.B/C', address: 'x:1', headers: {}, body: '{}', plaintext: true, options: { 'max-time': '30' } });
    expect(plan.options.timeout).toBe('30');
    expect(plan.adjusted).toEqual([]);
  });
});
