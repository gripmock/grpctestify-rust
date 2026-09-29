import { describe, it, expect } from 'vitest';
import { reflectOutcome, schemaKey, shouldAskServer } from './reflect-outcome';

const base = { aborted: false, timedOut: false, superseded: false, hadMethods: false };

describe('reflectOutcome', () => {
  it('leaves the state alone when a newer attempt has taken over', () => {
    expect(reflectOutcome({ ...base, superseded: true, aborted: true, hadMethods: true }))
      .toEqual({ status: 'loading', error: null, clearMethods: false });
  });

  it('treats a cancelled attempt as no news, keeping the methods it had', () => {
    expect(reflectOutcome({ ...base, aborted: true, hadMethods: true }))
      .toEqual({ status: 'ok', error: null, clearMethods: false });
    expect(reflectOutcome({ ...base, aborted: true }))
      .toEqual({ status: 'idle', error: null, clearMethods: false });
  });

  it('says the deadline passed rather than blaming the network', () => {
    const out = reflectOutcome({ ...base, aborted: true, timedOut: true, seconds: 30 });
    expect(out.status).toBe('error');
    expect(out.error).toBe('no answer in 30 s');
  });

  it('reports what the server answered when it is not a 2xx', () => {
    const out = reflectOutcome({ ...base, ok: false, status: 502, statusText: 'Bad Gateway' });
    expect(out.error).toBe('the server answered 502 Bad Gateway');
    expect(out.clearMethods).toBe(true);
  });

  it('passes a transport failure through in its own words', () => {
    expect(reflectOutcome({ ...base, transportError: 'Failed to fetch' }).error).toBe('Failed to fetch');
  });

  it('prefers the reason the server itself gave', () => {
    expect(reflectOutcome({ ...base, ok: true, reported: 'unknown service' }).error).toBe('unknown service');
  });

  it('calls an empty reflection what it is', () => {
    expect(reflectOutcome({ ...base, ok: true, methodCount: 0 }).error).toContain('the server reflected no methods');
    expect(reflectOutcome({ ...base, ok: true, methodCount: 0 }).error).toContain('PROTO descriptor');
  });

  it('is a plain success when methods came back', () => {
    expect(reflectOutcome({ ...base, ok: true, methodCount: 12 }))
      .toEqual({ status: 'ok', error: null, clearMethods: false });
  });
});

describe('shouldAskServer', () => {
  const here = schemaKey({ address: 'localhost:4770', protocol: 'grpc' });
  const base = { address: 'localhost:4770', key: here, askedFor: null, status: 'idle' as const };

  it('asks the first time a list is opened', () => {
    expect(shouldAskServer(base)).toBe(true);
  });

  it('asks again once the workbench points somewhere else', () => {
    const elsewhere = schemaKey({ address: 'staging:4770', protocol: 'grpc' });
    expect(shouldAskServer({ ...base, askedFor: elsewhere, status: 'ok' })).toBe(true);
  });

  it('asks again once the transport changes under the same address', () => {
    const overWeb = schemaKey({ address: 'localhost:4770', protocol: 'grpc-web' });
    expect(shouldAskServer({ ...base, key: overWeb, askedFor: here, status: 'ok' })).toBe(true);
  });

  it('does not ask twice for the same server, transport and file', () => {
    expect(shouldAskServer({ ...base, askedFor: here, status: 'ok' })).toBe(false);
    expect(shouldAskServer({ ...base, askedFor: here, status: 'error' })).toBe(false);
  });

  it('does not interrupt one already in flight', () => {
    expect(shouldAskServer({ ...base, status: 'loading' })).toBe(false);
  });

  it('has nothing to ask with no address', () => {
    expect(shouldAskServer({ ...base, address: '  ' })).toBe(false);
  });
});

describe('schemaKey', () => {
  it('is a different list for a different transport', () => {
    const grpc = schemaKey({ address: 'localhost:4770', protocol: 'grpc' });
    const web = schemaKey({ address: 'localhost:4770', protocol: 'grpc-web' });
    expect(grpc).not.toBe(web);
  });

  it('is a different list for a different file, because a PROTO section answers instead', () => {
    expect(schemaKey({ address: 'a:1', protocol: 'grpc', collectionPath: 'x.gctf' }))
      .not.toBe(schemaKey({ address: 'a:1', protocol: 'grpc', collectionPath: 'y.gctf' }));
  });

  it('is the same list for the same server, transport and file', () => {
    expect(schemaKey({ address: ' a:1 ', protocol: 'grpc', collectionPath: null }))
      .toBe(schemaKey({ address: 'a:1', protocol: 'grpc' }));
  });
});

describe('whether a first look asks', () => {
  it('asks for the address a call would dial, however it was arrived at', () => {
    expect(shouldAskServer({
      address: 'localhost:4770', askedFor: null, status: 'idle',
      key: schemaKey({ address: 'localhost:4770', protocol: 'grpc' }),
    })).toBe(true);
  });

  it('still asks nothing when there is no target at all', () => {
    expect(shouldAskServer({ address: '', askedFor: null, status: 'idle', key: '|grpc|' })).toBe(false);
  });
});
