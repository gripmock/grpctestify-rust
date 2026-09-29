import { describe, it, expect } from 'vitest';
import { claimToken, hasToken, noteUnauthorized, streamUrl, subscribeUnauthorized, tokenFromUrl, tokenRejected, withToken } from './play-token';

describe('the token a network bind needs', () => {
  it('comes out of the link the server printed, and out of the address bar', () => {
    expect(tokenFromUrl('http://host:8871/?token=abc')).toEqual({ token: 'abc', url: 'http://host:8871/' });
    expect(tokenFromUrl('http://host:8871/c/a.gctf?token=abc&x=1'))
      .toEqual({ token: 'abc', url: 'http://host:8871/c/a.gctf?x=1' });
    expect(tokenFromUrl('http://host:8871/c/a.gctf')).toEqual({ token: null, url: 'http://host:8871/c/a.gctf' });
  });

  it('is kept for the session and read back on the next page', () => {
    const store = new Map<string, string>();
    const storage = {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => { store.set(k, v); },
    } as unknown as Storage;
    let replaced = '';

    expect(claimToken(storage, { href: 'http://h/?token=abc' }, url => { replaced = url; })).toBe('abc');
    expect(replaced).toBe('http://h/');
    expect(claimToken(storage, { href: 'http://h/' }, () => {})).toBe('abc');
  });

  it('rides on every request, and on the one url that cannot carry a header', () => {
    const init = withToken('abc', { method: 'POST' })!;
    expect(new Headers(init.headers).get('Authorization')).toBe('Bearer abc');
    expect(streamUrl('abc', '/api/jobs/1/events')).toBe('/api/jobs/1/events?token=abc');
    expect(streamUrl('abc', '/api/x?y=1')).toBe('/api/x?y=1&token=abc');
  });

  it('is nothing at all on a loopback workbench', () => {
    expect(withToken(null, { method: 'GET' })).toEqual({ method: 'GET' });
    expect(streamUrl(null, '/api/x')).toBe('/api/x');
  });
});

describe('a token that stopped working', () => {
  it('is said once, to whoever is listening', () => {
    let told = 0;
    const stop = subscribeUnauthorized(() => { told += 1; });
    expect(tokenRejected()).toBe(false);

    noteUnauthorized();
    noteUnauthorized();
    expect(told).toBe(1);
    expect(tokenRejected()).toBe(true);
    stop();
  });
});

describe('whether this workbench needs a token at all', () => {
  it('follows what was claimed at startup', () => {
    const empty = new Map<string, string>();
    const storage = {
      getItem: (k: string) => empty.get(k) ?? null,
      setItem: (k: string, v: string) => { empty.set(k, v); },
    } as unknown as Storage;

    claimToken(storage, { href: 'http://h/' }, () => {});
    expect(hasToken()).toBe(false);

    claimToken(storage, { href: 'http://h/?token=abc' }, () => {});
    expect(hasToken()).toBe(true);
  });
});
