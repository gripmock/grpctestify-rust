import { describe, it, expect } from 'vitest';
import { compressionFromFile, connectionFromFile, connectionUsed, fileConnectionNote, timeoutUsed } from './connection-source';
import type { CollectionParsed } from './types';

function parsed(over: Partial<CollectionParsed> = {}): CollectionParsed {
  return {
    endpoint: 'pkg.Svc/M', address: '', headers: {}, bodies: ['{}'], asserts: [], extracts: {},
    meta_name: null, meta_tags: [], meta_owner: null, meta_summary: null, meta_links: [],
    tls: {}, options: {}, bench: {}, proto: {}, dataset: [], attributes: [],
    expect_responses: [], expect_error: null,
    ...over,
  };
}

const grpcPlain = { protocol: 'grpc' as const, tls: false, tlsInsecure: true };

describe('connectionFromFile', () => {
  it('says nothing when the file says nothing', () => {
    expect(connectionFromFile(parsed(), grpcPlain)).toEqual({ protocol: null, tls: null });
    expect(connectionFromFile(null, grpcPlain)).toEqual({ protocol: null, tls: null });
  });

  it('reports a protocol the file names, and whether the header agrees', () => {
    const away = connectionFromFile(parsed({ options: { protocol: 'grpc-web' } }), grpcPlain);
    expect(away.protocol).toEqual({ value: 'grpc-web', differs: true });

    const same = connectionFromFile(parsed({ options: { protocol: 'grpc' } }), grpcPlain);
    expect(same.protocol).toEqual({ value: 'grpc', differs: false });
  });

  it('ignores a protocol value that is not one', () => {
    expect(connectionFromFile(parsed({ options: { protocol: 'carrier pigeon' } }), grpcPlain).protocol).toBeNull();
  });

  it('reads the TLS section as the mode it means', () => {
    expect(connectionFromFile(parsed({ tls: { ca: '/ca.pem' } }), grpcPlain).tls)
      .toEqual({ value: 'tls', differs: true });
    expect(connectionFromFile(parsed({ tls: { insecure: 'true' } }), { ...grpcPlain, tls: true, tlsInsecure: true }).tls)
      .toEqual({ value: 'insecure', differs: false });
  });
});

describe('fileConnectionNote', () => {
  it('names each half the file carries', () => {
    const from = connectionFromFile(parsed({ options: { protocol: 'connectrpc' }, tls: { ca: '/ca.pem' } }), grpcPlain);
    expect(fileConnectionNote(from)).toBe('OPTIONS.protocol: connectrpc · TLS: tls');
    expect(fileConnectionNote({ protocol: null, tls: null })).toBe('');
  });
});

describe('the connection a call goes out on', () => {
  const client = { protocol: 'grpc' as const, tls: false, tlsInsecure: true };
  const file = (over: any) => ({ options: {}, tls: {}, ...over }) as any;

  it('is the workbench when the file names nothing', () => {
    expect(connectionUsed(null, client)).toEqual(client);
    expect(connectionUsed(file({}), client)).toEqual(client);
  });

  it('is the file where the file speaks', () => {
    expect(connectionUsed(file({ options: { protocol: 'grpc-web' } }), client).protocol).toBe('grpc-web');
    expect(connectionUsed(file({ tls: { insecure: 'true' } }), client)).toMatchObject({ tls: true, tlsInsecure: true });
    expect(connectionUsed(file({ tls: { ca_cert: '/ca.pem' } }), client)).toMatchObject({ tls: true, tlsInsecure: false });
  });
});

describe('the wait a call actually takes', () => {
  const parsedWith = (options: Record<string, string>) => parsed({ options });

  it('is the file’s own OPTIONS timeout when it has one', () => {
    expect(timeoutUsed(parsedWith({ timeout: '5' }), 30_000))
      .toEqual({ seconds: 5, source: 'file', from: 'options' });
  });

  it('is the workbench’s wait where the file is silent', () => {
    expect(timeoutUsed(parsedWith({ retry: '2' }), 12_000)).toEqual({ seconds: 12, source: 'workbench' });
    expect(timeoutUsed(null, 12_000)).toEqual({ seconds: 12, source: 'workbench' });
  });

  it('is thirty seconds when nothing says otherwise — an emptied box is not “wait forever”', () => {
    expect(timeoutUsed(null, 0)).toEqual({ seconds: 30, source: 'default' });
    expect(timeoutUsed(parsedWith({ timeout: '0' }), 0)).toEqual({ seconds: 30, source: 'default' });
  });
});

describe('what a call compresses', () => {
  it('is gzip when the file asks for it', () => {
    expect(compressionFromFile(parsed({ options: { compression: 'gzip' } }))).toBe('gzip');
  });

  it('is nothing when the file is silent or says none', () => {
    expect(compressionFromFile(parsed({ options: { compression: 'none' } }))).toBeNull();
    expect(compressionFromFile(parsed())).toBeNull();
    expect(compressionFromFile(null)).toBeNull();
  });
});

describe('a section attribute outranks the OPTIONS line', () => {
  it('bounds the wait at what the section asks for', () => {
    expect(timeoutUsed(parsed({ options: { timeout: '30' }, attributes: [{ section: 'REQUEST', index: 0, name: 'timeout', value: '5' }] }), 0))
      .toEqual({ seconds: 5, source: 'file', from: 'attribute' });
  });

  it('compresses when the section asks and the OPTIONS line does not', () => {
    expect(compressionFromFile(parsed({ options: { compression: 'none' }, attributes: [{ section: 'REQUEST', index: 0, name: 'compression', value: 'gzip' }] })))
      .toBe('gzip');
  });
});
