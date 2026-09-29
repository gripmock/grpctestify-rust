import { describe, it, expect } from 'vitest';
import { addressForSave, metaFromParsed, protocolForSave } from './save-meta';
import type { CollectionParsed } from './types';

function parsed(over: Partial<CollectionParsed> = {}): CollectionParsed {
  return {
    endpoint: 'a.A/One', address: '', headers: {}, bodies: ['{}'],
    asserts: [], extracts: {}, meta_name: null, meta_tags: [], meta_owner: null, meta_summary: null, meta_links: [],
    tls: {}, options: {}, bench: {}, proto: {}, dataset: [], attributes: [],
    expect_responses: [], expect_error: null,
    ...over,
  };
}

describe('metaFromParsed', () => {
  it('carries the links a file already has', () => {
    expect(metaFromParsed(parsed({ meta_links: ['https://t/1'] })).links).toEqual(['https://t/1']);
  });

  it('starts empty for a file that has none', () => {
    expect(metaFromParsed(null)).toEqual({ name: undefined, summary: undefined, owner: undefined, tags: [], links: [] });
  });
});

describe('addressForSave', () => {
  it('writes what was typed for a file that does not exist yet', () => {
    expect(addressForSave(null, 'localhost:4770', false)).toBe('localhost:4770');
  });

  it('keeps the file\'s own when nothing was typed for it', () => {
    expect(addressForSave(parsed({ address: 'localhost:4770' }), 'other:9000', false)).toBe('localhost:4770');
  });

  it('writes what was typed once it was typed for this file', () => {
    expect(addressForSave(parsed({ address: 'localhost:4770' }), 'staging:4770', true)).toBe('staging:4770');
  });

  it('leaves a file that dials the environment alone', () => {
    expect(addressForSave(parsed(), 'somewhere-else:4770', false)).toBeUndefined();
  });

  it('takes the address off a file when the field is cleared for it', () => {
    expect(addressForSave(parsed({ address: 'localhost:4770' }), '   ', true)).toBeUndefined();
  });
});

describe('protocolForSave', () => {
  const withProtocol = (value?: string) =>
    parsed(value === undefined ? {} : { options: { protocol: value } });

  it('keeps the file\'s own transport when nobody chose another for it', () => {
    expect(protocolForSave(withProtocol('grpc-web'), 'grpc', false)).toBe('grpc-web');
  });

  it('takes what was chosen for this file', () => {
    expect(protocolForSave(withProtocol('grpc-web'), 'connectrpc', true)).toBe('connectrpc');
  });

  it('writes nothing for the default', () => {
    expect(protocolForSave(withProtocol('grpc-web'), 'grpc', true)).toBeUndefined();
    expect(protocolForSave(null, 'grpc', false)).toBeUndefined();
  });

  it('writes what a file that does not exist yet was set to', () => {
    expect(protocolForSave(null, 'grpc-web', false)).toBe('grpc-web');
  });

  it('leaves a file that names no transport naming none', () => {
    expect(protocolForSave(withProtocol(), 'grpc-web', false)).toBeUndefined();
    expect(protocolForSave(withProtocol(), 'connectrpc', false)).toBeUndefined();
  });

  it('still writes one chosen for this file', () => {
    expect(protocolForSave(withProtocol(), 'grpc-web', true)).toBe('grpc-web');
  });
});
