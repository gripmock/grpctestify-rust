import { describe, it, expect } from 'vitest';
import { matchingPages, pageTitle, pageForHref, runsTheWholeFile } from './docs';

describe('a documentation page', () => {
  it('is named for the service it is about', () => {
    expect(pageTitle('auth.v1.AuthService.md')).toBe('auth.v1.AuthService');
    expect(pageTitle('index.md')).toBe('overview');
    expect(pageTitle('v1.md', '# /v1\n\n## users\n')).toBe('/v1');
    expect(pageTitle('pkg.Svc.md', '# pkg.Svc\n')).toBe('pkg.Svc');
    expect(pageTitle('v1.md')).toBe('v1');
  });
});

describe('a link between doc pages', () => {
  const pages = [
    { name: 'index.md', markdown: '' },
    { name: 'auth.v1.AuthService.md', markdown: '' },
  ];

  it('is the page it names', () => {
    expect(pageForHref(pages, 'auth.v1.AuthService.md')).toBe(1);
    expect(pageForHref(pages, './auth.v1.AuthService.md#methods')).toBe(1);
    expect(pageForHref(pages, 'index.md')).toBe(0);
  });

  it('is nothing when it points outside them', () => {
    expect(pageForHref(pages, 'https://example.org/docs')).toBe(-1);
    expect(pageForHref(pages, 'users.v1.UsersService.md')).toBe(-1);
  });
});

describe('a call line that runs a file', () => {
  it('is the one with no inline endpoint', () => {
    expect(runsTheWholeFile("grpctestify call '.grpctestify/collections/greet.gctf'")).toBe(true);
    expect(runsTheWholeFile("grpctestify call 'chain.gctf' --doc-index 2")).toBe(true);
  });

  it('is not an inline request', () => {
    expect(runsTheWholeFile("grpctestify call -e 'a.B/C' -d '{}'")).toBe(false);
  });

  it('is not some other command', () => {
    expect(runsTheWholeFile("grpcurl -plaintext 'localhost:1' 'a.B/C'")).toBe(false);
    expect(runsTheWholeFile('')).toBe(false);
  });
});

describe('the pages a query names', () => {
  const pages = [
    { name: 'index.md', markdown: '# API Documentation\n' },
    { name: 'v1.md', markdown: '# /v1\n\n## users\n\nGET /v1/users\n' },
    { name: 'pkg.Svc.md', markdown: '# pkg.Svc\n\n## greeting\n\nSayHello\n' },
  ];

  it('matches the title', () => {
    expect(matchingPages(pages, 'v1').map(p => p.name)).toEqual(['index.md', 'v1.md']);
  });

  it('matches the text when the title does not', () => {
    expect(matchingPages(pages, 'SayHello').map(p => p.name)).toEqual(['index.md', 'pkg.Svc.md']);
  });

  it('keeps the index, which is the way back to everything', () => {
    expect(matchingPages(pages, 'nothing-here').map(p => p.name)).toEqual(['index.md']);
  });

  it('is every page when nothing is typed', () => {
    expect(matchingPages(pages, '  ')).toHaveLength(3);
  });
});
