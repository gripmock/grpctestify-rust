import { describe, it, expect } from 'vitest';
import { parseDeepLink, encodeCollectionLink } from './deeplink';

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
