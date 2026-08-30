import { describe, it, expect } from 'vitest';
import { parseDeepLink, encodeCollectionLink, nextUrl, urlWhenLinkFails } from './deeplink';

describe('parseDeepLink', () => {
  it('parses a simple collection path', () => {
    expect(parseDeepLink('/c/test.gctf')).toEqual({ kind: 'collection', value: 'test.gctf' });
  });

  it('decodes URI-encoded collection paths (spaces, slashes)', () => {
    const path = 'dir with space/sub/My Test.gctf';
    expect(parseDeepLink(encodeCollectionLink(path))).toEqual({ kind: 'collection', value: path });
  });

  it('round-trips encode/decode for nested paths', () => {
    const path = 'a/b/c.gctf';
    const link = encodeCollectionLink(path);
    expect(link).toBe('/c/a%2Fb%2Fc.gctf');
    expect(parseDeepLink(link)).toEqual({ kind: 'collection', value: path });
  });

  it('parses and decodes share links', () => {
    expect(parseDeepLink('/s/abc-123')).toEqual({ kind: 'share', value: 'abc-123' });
  });

  it('returns null for unknown paths', () => {
    expect(parseDeepLink('/')).toBeNull();
    expect(parseDeepLink('/other')).toBeNull();
  });
});

describe('nextUrl', () => {
  it('names the open file', () => {
    expect(nextUrl('/', 'examples/basic/hello.gctf', null)).toBe('/c/examples%2Fbasic%2Fhello.gctf');
  });

  it('goes back to the root when nothing is open', () => {
    expect(nextUrl('/c/examples%2Fbasic%2Fhello.gctf', null, null)).toBe('/');
  });

  it('leaves a url that already says this', () => {
    expect(nextUrl('/c/a.gctf', 'a.gctf', null)).toBeNull();
  });

  it('leaves a share alone while it opens', () => {
    expect(nextUrl('/s/abc123', 'a.gctf', null)).toBeNull();
  });

  it('waits for the file a deep link named', () => {
    expect(nextUrl('/c/b.gctf', 'a.gctf', 'b.gctf')).toBeNull();
    expect(nextUrl('/c/b.gctf', 'b.gctf', 'b.gctf')).toBeNull();
    expect(nextUrl('/', 'b.gctf', 'b.gctf')).toBe('/c/b.gctf');
  });
});

describe('the address bar after a link that could not be opened', () => {
  it('names whatever is open instead', () => {
    expect(urlWhenLinkFails('auth/login.gctf')).toBe(encodeCollectionLink('auth/login.gctf'));
  });

  it('is the root when nothing is open', () => {
    expect(urlWhenLinkFails(null)).toBe('/');
  });
});
