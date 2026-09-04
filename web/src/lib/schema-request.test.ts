import { describe, it, expect } from 'vitest';
import { schemaRequest } from './schema-request';

const base = {
  address: 'localhost:4770',
  protocol: 'grpc',
  selectedCollection: 'a/b.gctf',
  request: { endpoint: 'pkg.Svc/M' },
  tls: false, tlsInsecure: true, tlsCa: 'ca.pem', tlsCert: '', tlsKey: '',
  activeEnvironment: null, environments: [], serverEnv: {},
} as any;

describe('the descriptors question', () => {
  it('carries the connection and the file', () => {
    expect(schemaRequest(base)).toEqual({
      address: 'localhost:4770',
      endpoint: 'pkg.Svc/M',
      tls: undefined, tls_insecure: false, tls_ca: undefined, tls_cert: undefined, tls_key: undefined,
      collection_path: 'a/b.gctf',
      protocol: 'grpc',
    });
  });

  it('says outright that certificates are verified, rather than leaving it to the server', () => {
    expect(schemaRequest({ ...base, tls: false, tlsInsecure: true }).tls_insecure).toBe(false);
    expect(schemaRequest({ ...base, tls: true, tlsInsecure: false }).tls_insecure).toBe(false);
    expect(schemaRequest({ ...base, tls: true, tlsInsecure: true }).tls_insecure).toBe(true);
  });

  it('sends the TLS material only when TLS is on', () => {
    const on = schemaRequest({ ...base, tls: true });
    expect(on.tls).toBe(true);
    expect(on.tls_ca).toBe('ca.pem');
    expect(on.tls_cert).toBeUndefined();
  });

  it('takes TLS from the active environment, as a call does', () => {
    const env = { ...base, activeEnvironment: 'dev', environments: [{ name: 'dev', variables: {}, tls: true, tlsInsecure: false }] };
    expect(schemaRequest(env)).toMatchObject({ tls: true, tls_insecure: false });
  });

  it('asks about the endpoint it is given, not only the open one', () => {
    expect(schemaRequest(base, 'other.Svc/N').endpoint).toBe('other.Svc/N');
  });
});

describe('the wait the schema question carries', () => {
  const state = (ms: number) => ({ ...base, requestTimeoutMs: ms }) as any;
  it('is the workbench’s own', () => {
    expect(schemaRequest(state(4000)).timeout_seconds).toBe(4);
  });

  it('is left out when the box is empty, so the server keeps its default', () => {
    expect(schemaRequest(state(0)).timeout_seconds).toBeUndefined();
  });
});
