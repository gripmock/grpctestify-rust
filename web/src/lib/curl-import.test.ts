import { describe, expect, it } from 'vitest';
import { curlSummary, isCurl, parseCurl, splitUrl, toCurl } from './curl-import';
import { parseShell } from './shell';
import { httpUrl, splitEndpoint } from './http-endpoint';

const of = (command: string) => parseCurl(parseShell(command));

describe('a curl command', () => {
  it('is recognised by its own name', () => {
    expect(isCurl('curl https://api.example.com')).toBe(true);
    expect(isCurl('  curl -X POST /x')).toBe(true);
    expect(isCurl('grpcurl -plaintext localhost:4770 a.B/C')).toBe(false);
  });

  it('is a GET of a url when it is nothing else', () => {
    const c = of('curl https://api.example.com/v1/users');
    expect(c.method).toBe('GET');
    expect(c.address).toBe('https://api.example.com');
    expect(c.path).toBe('/v1/users');
    expect(c.body).toBe('');
  });

  it('carries its method, its headers and its body', () => {
    const c = of(`curl -L -X POST 'https://api.example.com/v1/users' -H 'content-type: application/json' -H 'authorization: Bearer t0ken' -d '{"name":"Ada"}'`);
    expect(c.method).toBe('POST');
    expect(c.path).toBe('/v1/users');
    expect(c.headers).toEqual({ 'content-type': 'application/json', authorization: 'Bearer t0ken' });
    expect(c.body).toBe('{"name":"Ada"}');
  });

  it('is a POST when it has a body and nobody said otherwise', () => {
    expect(of("curl https://api.example.com/x -d 'a=1'").method).toBe('POST');
    expect(of("curl -X PUT https://api.example.com/x -d 'a=1'").method).toBe('PUT');
  });

  it('joins the several data flags curl allows', () => {
    expect(of("curl https://x.test/f -d 'a=1' -d 'b=2'").body).toBe('a=1&b=2');
  });

  it('takes --json as a body and the content type it implies', () => {
    const c = of(`curl --json '{"a":1}' https://x.test/f`);
    expect(c.body).toBe('{"a":1}');
    expect(c.headers['content-type']).toBe('application/json');
  });

  it('reads the url from --url as readily as from the bare argument', () => {
    expect(of('curl --url https://x.test/a/b').path).toBe('/a/b');
  });

  it('names what it could not bring rather than dropping it', () => {
    const c = of("curl https://x.test/f -u me:secret --max-time 5 --compressed");
    expect(c.ignored).toContain('-u');
    expect(c.ignored).toContain('--max-time 5');
    expect(c.ignored).not.toContain('--compressed');
  });

  it('says when the command turned certificate checks off', () => {
    expect(of('curl -k https://x.test/f').insecure).toBe(true);
    expect(of('curl https://x.test/f').insecure).toBe(false);
  });

  it('survives a command with no url at all', () => {
    const c = of('curl -X POST');
    expect(c.address).toBe('');
    expect(c.path).toBe('');
  });
});

describe('a url', () => {
  it('splits into the address a file dials and the path it asks for', () => {
    expect(splitUrl('https://api.example.com/v1/users?full=true'))
      .toEqual({ address: 'https://api.example.com', path: '/v1/users?full=true' });
    expect(splitUrl('http://localhost:8080')).toEqual({ address: 'http://localhost:8080', path: '/' });
  });

  it('leaves a bare path as a path', () => {
    expect(splitUrl('/v1/users')).toEqual({ address: '', path: '/v1/users' });
  });
});

describe('what the import says it did', () => {
  it('names the call, the target and what came with it', () => {
    const lines = curlSummary(of(`curl -X POST https://x.test/f -H 'a: b' -d '{"n":1}'`));
    expect(lines[0]).toBe('POST /f');
    expect(lines).toContain('address https://x.test');
    expect(lines).toContain('1 header');
  });
});

describe('the curl a request writes out', () => {
  it('is the command that would make the same call', () => {
    expect(toCurl({
      method: 'POST',
      url: 'https://api.example.com/v1/users',
      headers: { 'content-type': 'application/json' },
      body: '{"name":"Ada"}',
    })).toBe(`curl -L -X POST 'https://api.example.com/v1/users' -H 'content-type: application/json' -d '{"name":"Ada"}'`);
  });

  it('leaves out what curl already assumes', () => {
    expect(toCurl({ method: 'GET', url: 'https://x.test/f', headers: {}, body: '' }))
      .toBe(`curl -L 'https://x.test/f'`);
  });

  it('quotes a value that would otherwise end the argument', () => {
    const line = toCurl({ method: 'POST', url: 'https://x.test/f', headers: {}, body: `it's fine` });
    expect(line).toContain(`'it'\\''s fine'`);
  });

  it('round-trips through the parser', () => {
    const line = toCurl({
      method: 'PUT',
      url: 'https://api.example.com/v1/users/7?full=true',
      headers: { authorization: 'Bearer t0ken' },
      body: '{"name":"Grace"}',
    });
    const back = parseCurl(parseShell(line));
    expect(back.method).toBe('PUT');
    expect(back.address).toBe('https://api.example.com');
    expect(back.path).toBe('/v1/users/7?full=true');
    expect(back.headers).toEqual({ authorization: 'Bearer t0ken' });
    expect(back.body).toBe('{"name":"Grace"}');
  });
});

describe('a recorded HTTP call as a command line', () => {
  it('is a curl line against the address it dialled', () => {
    const { method, path } = splitEndpoint('POST /v1/users');
    const line = toCurl({
      method,
      url: httpUrl('http://127.0.0.1:8899', path),
      headers: { authorization: 'Bearer t' },
      body: '{"name":"Ada"}',
    });
    expect(line).toContain("curl -L -X POST 'http://127.0.0.1:8899/v1/users'");
    expect(line).toContain("-H 'authorization: Bearer t'");
    expect(line).toContain('{"name":"Ada"}');
  });
});

describe('which command was pasted', () => {
  it('reads the command, not the path in front of it', () => {
    expect(isCurl('curl https://api.example.com')).toBe(true);
    expect(isCurl('/usr/bin/curl https://api.example.com')).toBe(true);
    expect(isCurl('$ curl https://api.example.com')).toBe(true);
    expect(isCurl('  curl -X POST https://api.example.com')).toBe(true);
  });

  it('is not fooled by a name that merely starts the same', () => {
    expect(isCurl('curlie https://api.example.com')).toBe(false);
    expect(isCurl('grpcurl -plaintext localhost:4770 list')).toBe(false);
    expect(isCurl('/usr/local/bin/grpcurl -plaintext localhost:4770 list')).toBe(false);
  });
});

describe('a curl line that names its binary by path', () => {
  it('is parsed like any other', () => {
    const bare = parseCurl(parseShell("curl -L -X POST 'http://127.0.0.1:8899/echo' -d '{\"a\":1}'"));
    const pathy = parseCurl(parseShell("/usr/bin/curl -X POST 'http://127.0.0.1:8899/echo' -d '{\"a\":1}'"));
    expect(pathy).toEqual(bare);
    expect(pathy.address).toBe('http://127.0.0.1:8899');
    expect(pathy.path).toBe('/echo');
  });
});

describe('what a pasted command must not carry into the workbench', () => {
  it('names a credential flag without its value', () => {
    const out = parseCurl(parseShell("curl -u ada:hunter2 -b 'session=abc' https://api.example.com/v1/users"));
    expect(out.ignored).toContain('-u');
    expect(out.ignored).toContain('-b');
    expect(out.ignored.join(' ')).not.toContain('hunter2');
    expect(out.ignored.join(' ')).not.toContain('session=abc');
  });

  it('does not fabricate a multipart body', () => {
    const out = parseCurl(parseShell("curl -F name=Ada -F file=@/tmp/x.png https://api.example.com/upload"));
    expect(out.body).toBe('');
    expect(out.headers['content-type']).toBeUndefined();
    expect(out.ignored.some(i => i.startsWith('-F'))).toBe(true);
  });

  it('still imports an ordinary form body', () => {
    const out = parseCurl(parseShell("curl -X POST -d 'name=Ada&role=eng' https://api.example.com/v1/users"));
    expect(out.body).toBe('name=Ada&role=eng');
  });
});

describe('a command the workbench writes and reads back', () => {
  it('round-trips a body that contains quotes', () => {
    const line = toCurl({
      method: 'POST',
      url: 'http://127.0.0.1:8899/echo',
      headers: { 'content-type': 'application/json' },
      body: `{"note":"it's fine","q":"say \\"hi\\""}`,
    });
    const back = parseCurl(parseShell(line));
    expect(back.method).toBe('POST');
    expect(back.address).toBe('http://127.0.0.1:8899');
    expect(back.path).toBe('/echo');
    expect(back.headers['content-type']).toBe('application/json');
    expect(back.body).toBe(`{"note":"it's fine","q":"say \\"hi\\""}`);
  });
});

describe('a curl that puts its data in the query string', () => {
  it('keeps the method and moves the data into the path', () => {
    const c = parseCurl(['curl', '-G', 'https://api.example.com/search', '-d', 'q=ada', '-d', 'page=2']);
    expect(c.method).toBe('GET');
    expect(c.path).toBe('/search?q=ada&page=2');
    expect(c.body).toBe('');
    expect(c.ignored).not.toContain('-G');
  });

  it('appends to a query the url already has', () => {
    const c = parseCurl(['curl', '--get', 'https://api.example.com/search?lang=en', '-d', 'q=ada']);
    expect(c.path).toBe('/search?lang=en&q=ada');
  });

  it('leaves an explicit method alone', () => {
    const c = parseCurl(['curl', '-G', '-X', 'HEAD', 'https://api.example.com/x', '-d', 'a=1']);
    expect(c.method).toBe('HEAD');
    expect(c.path).toBe('/x?a=1');
  });
});

describe('flags that eat the word after them', () => {
  it('does not read an output filename as the address', () => {
    const c = parseCurl(['curl', '-o', 'out.json', 'https://api.example.com/x']);
    expect(c.address).toBe('https://api.example.com');
    expect(c.path).toBe('/x');
  });

  it('names a credential flag without its value', () => {
    const c = parseCurl(['curl', '--oauth2-bearer', 'tok_secret', 'https://api.example.com/x']);
    expect(c.ignored).toContain('--oauth2-bearer');
    expect(c.ignored.join(' ')).not.toContain('tok_secret');
  });

  it('reads -I as the method it is', () => {
    const c = parseCurl(['curl', '-I', 'https://api.example.com/x']);
    expect(c.method).toBe('HEAD');
    expect(c.body).toBe('');
  });

  it('reads an upload as a PUT and says the file did not come', () => {
    const c = parseCurl(['curl', '-T', 'photo.png', 'https://api.example.com/upload']);
    expect(c.method).toBe('PUT');
    expect(c.address).toBe('https://api.example.com');
    expect(c.path).toBe('/upload');
    expect(c.ignored.join(' ')).toContain('the file itself is not imported');
  });
});

describe('a copied curl and the call it stands for', () => {
  it('follows redirects, because the workbench did', () => {
    const line = toCurl({ method: 'GET', url: 'https://x.test/old', headers: {}, body: '' });
    expect(line.startsWith('curl -L ')).toBe(true);
  });

  it('round-trips through the importer as the same request', () => {
    const line = toCurl({ method: 'GET', url: 'https://x.test/old', headers: {}, body: '' });
    const back = parseCurl(parseShell(line));
    expect(back.method).toBe('GET');
    expect(back.address).toBe('https://x.test');
    expect(back.path).toBe('/old');
    expect(back.ignored).toEqual([]);
  });
});

describe('a body curl encodes on the way out', () => {
  it('encodes the value and keeps the name', () => {
    const said = of("curl https://api.test/s --data-urlencode 'q=a b&c'");
    expect(said.body).toBe('q=a%20b%26c');
    expect(said.method).toBe('POST');
  });

  it('encodes the whole thing when there is no name', () => {
    expect(of("curl https://api.test/s --data-urlencode 'a b'").body).toBe('a%20b');
    expect(of("curl https://api.test/s --data-urlencode '=a b'").body).toBe('a%20b');
  });

  it('joins several the way curl joins them', () => {
    const said = of("curl https://api.test/s --data-urlencode 'a=1 2' --data-urlencode 'b=3'");
    expect(said.body).toBe('a=1%202&b=3');
  });

  it('says when the value comes from a file', () => {
    const said = of("curl https://api.test/s --data-urlencode 'q@query.txt'");
    expect(said.body).toBe('');
    expect(said.ignored.join(' ')).toContain('not imported');
  });

  it('leaves a plain -d as it was written', () => {
    expect(of("curl https://api.test/s -d 'q=a b'").body).toBe('q=a b');
  });
});
