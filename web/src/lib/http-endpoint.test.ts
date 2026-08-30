import { describe, expect, it } from 'vitest';
import { joinEndpoint, noHostYet, pathIssue, splitEndpoint, isHttpRequest, looksHttp, httpUrl, draftFileName, methodTone, suggestedFileName, shortTarget } from './http-endpoint';

describe('an http endpoint', () => {
  it('is a method and a path in one line', () => {
    expect(splitEndpoint('POST /v1/users')).toEqual({ method: 'POST', path: '/v1/users' });
    expect(joinEndpoint('post', ' /v1/users ')).toBe('POST /v1/users');
  });

  it('keeps a method this tool has never heard of', () => {
    expect(splitEndpoint('PROPFIND /dav/')).toEqual({ method: 'PROPFIND', path: '/dav/' });
  });

  it('survives a line that is only half written', () => {
    expect(splitEndpoint('GET')).toEqual({ method: 'GET', path: '' });
    expect(splitEndpoint('')).toEqual({ method: '', path: '' });
    expect(joinEndpoint('GET', '')).toBe('GET');
    expect(joinEndpoint('', '/x')).toBe('/x');
  });

  it('keeps the query and the placeholders of a path', () => {
    expect(splitEndpoint('GET /v1/users/{{id}}?full=true').path).toBe('/v1/users/{{id}}?full=true');
  });

  it('says what is wrong with a path, and nothing when it is fine', () => {
    expect(pathIssue('/v1/users')).toBeNull();
    expect(pathIssue('https://api.example.com/v1')).toBeNull();
    expect(pathIssue('')).toContain('missing');
    expect(pathIssue('v1/users')).toContain('starts with /');
  });
});

describe('what makes a request an HTTP one', () => {
  it('is the file, whenever there is one', () => {
    expect(isHttpRequest('a.httf', 'anything at all')).toBe(true);
    expect(isHttpRequest('a.gctf', 'GET /v1/users')).toBe(false);
  });

  it('is the shape of the endpoint when there is no file', () => {
    expect(isHttpRequest(null, 'GET /v1/users')).toBe(true);
    expect(isHttpRequest(null, 'PROPFIND /dav/')).toBe(true);
    expect(isHttpRequest(null, 'a.B/C')).toBe(false);
    expect(isHttpRequest(null, '')).toBe(false);
    expect(isHttpRequest(undefined, 'users.UserService/GetUser')).toBe(false);
  });

  it('is not fooled by a service and method, which never carry a space', () => {
    expect(looksHttp('users.UserService/GetUser')).toBe(false);
    expect(looksHttp('a.B/C')).toBe(false);
  });

  it('takes a method however it was typed', () => {
    expect(looksHttp('get /x')).toBe(true);
    expect(looksHttp('Post /v1/users')).toBe(true);
  });
});

describe('the url a request actually dials', () => {
  it('joins the address and the path', () => {
    expect(httpUrl('https://api.example.com', '/v1/users')).toBe('https://api.example.com/v1/users');
    expect(httpUrl('localhost:8080', '/health')).toBe('http://localhost:8080/health');
    expect(httpUrl('https://api.example.com/', 'v1/users')).toBe('https://api.example.com/v1/users');
  });

  it('leaves an absolute path alone', () => {
    expect(httpUrl('http://ignored', 'https://api.example.com/x')).toBe('https://api.example.com/x');
  });

  it('is the address alone when there is no path, and the path alone when there is no address', () => {
    expect(httpUrl('https://x.test', '')).toBe('https://x.test');
    expect(httpUrl('', '/v1/users')).toBe('/v1/users');
  });
});

describe('the name an unsaved tab is checked under', () => {
  it('follows the request, not the default', () => {
    expect(draftFileName(null, 'GET /v1/users')).toBe('playground.httf');
    expect(draftFileName(null, 'a.B/C')).toBe('playground.gctf');
    expect(draftFileName(null, '')).toBe('playground.gctf');
  });

  it('follows the file whenever there is one', () => {
    expect(draftFileName('api/users.httf', 'anything')).toBe('playground.httf');
    expect(draftFileName('api/users.gctf', 'GET /x')).toBe('playground.gctf');
  });
});

describe('what a method does', () => {
  it('is read from the method itself', () => {
    expect(methodTone('GET')).toBe('read');
    expect(methodTone('head')).toBe('read');
    expect(methodTone('POST')).toBe('write');
    expect(methodTone('PATCH')).toBe('write');
    expect(methodTone('DELETE')).toBe('destructive');
  });

  it('says nothing it does not know about a method it has never heard of', () => {
    expect(methodTone('PROPFIND')).toBe('other');
    expect(methodTone('')).toBe('other');
  });
});

describe('the name a request suggests for its file', () => {
  it('is the last part of the path, without the query', () => {
    expect(suggestedFileName('GET /v1/users?page=2')).toBe('users');
    expect(suggestedFileName('POST /v1/users')).toBe('users');
    expect(suggestedFileName('GET /v1/users/')).toBe('users');
  });

  it('falls back to the method when the path has no parts', () => {
    expect(suggestedFileName('GET /')).toBe('get');
    expect(suggestedFileName('DELETE /?x=1')).toBe('delete');
  });

  it('reads a variable as the name it stands for', () => {
    expect(suggestedFileName('GET /v1/users/{{user}}')).toBe('user');
  });

  it('is the method for a gRPC endpoint, as it was', () => {
    expect(suggestedFileName('auth.v1.AuthService/Login')).toBe('Login');
    expect(suggestedFileName('')).toBe('');
  });
});

describe('a path that begins with a variable', () => {
  it('is not a relative path', () => {
    expect(pathIssue('{{dataset.path}}')).toBeNull();
    expect(pathIssue('{{base}}/users')).toBeNull();
  });

  it('leaves the rest of the rule alone', () => {
    expect(pathIssue('v1/users')).toBe('a relative path starts with /');
    expect(pathIssue('')).toBe('a path is missing');
    expect(pathIssue('/v1/users')).toBeNull();
  });
});

describe('a target as a row shows it', () => {
  it('drops the scheme that says nothing', () => {
    expect(shortTarget('http://localhost:8871')).toBe('localhost:8871');
    expect(shortTarget('HTTP://api.test/v1')).toBe('api.test/v1');
  });

  it('keeps the one that says something', () => {
    expect(shortTarget('https://api.test')).toBe('https://api.test');
  });

  it('leaves a gRPC target as it is', () => {
    expect(shortTarget('localhost:4770')).toBe('localhost:4770');
    expect(shortTarget('')).toBe('');
    expect(shortTarget(null)).toBe('');
  });
});

describe('a command is not an HTTP request', () => {
  it('leaves a grpcurl line as one field', () => {
    expect(looksHttp("grpcurl -plaintext -d '{}' localhost:4770 a.B/C")).toBe(false);
    expect(isHttpRequest(null, "grpcurl -plaintext localhost:4770 a.B/C")).toBe(false);
  });

  it('leaves a curl line as one field', () => {
    expect(looksHttp('curl -X POST https://api.example.com/v1/users')).toBe(false);
  });

  it('knows it again once the field has capitalised it', () => {
    expect(looksHttp('GRPCURL -plaintext localhost:4770 a.B/C')).toBe(false);
  });

  it('leaves a method and a path alone', () => {
    expect(looksHttp('GET /v1/users')).toBe(true);
    expect(looksHttp('POST /v1/users?q=1')).toBe(true);
  });

  it('does not override the family a file name gives', () => {
    expect(isHttpRequest('probe.httf', 'curl https://example.com')).toBe(true);
  });
});

describe('a row with nowhere to send the call', () => {
  it('is an HTTP row with no address and a relative path', () => {
    expect(noHostYet(true, '', '/data.json')).toBe(true);
    expect(noHostYet(true, '   ', 'data.json')).toBe(true);
  });

  it('is not one that is aimed, however it is aimed', () => {
    expect(noHostYet(true, 'http://x.test', '/data.json')).toBe(false);
    expect(noHostYet(true, '', 'https://x.test/data.json')).toBe(false);
  });

  it('says nothing about a gRPC row, which has a port to fall back on', () => {
    expect(noHostYet(false, '', 'pkg.Svc/M')).toBe(false);
  });
});
