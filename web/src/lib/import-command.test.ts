import { describe, expect, it } from 'vitest';
import { importable } from './import-command';

describe('a command typed where an endpoint goes', () => {
  it('knows a grpcurl line', () => {
    expect(importable('grpcurl -plaintext localhost:4770 pkg.Svc/Method')).toBe('grpcurl');
  });

  it('knows a curl line', () => {
    expect(importable('curl -X POST https://api.example.com/v1/users')).toBe('curl');
  });

  it('reads a path and a prompt off the front', () => {
    expect(importable('$ /usr/bin/curl https://example.com')).toBe('curl');
    expect(importable('/usr/local/bin/grpcurl -plaintext host:1 a.B/C')).toBe('grpcurl');
    expect(importable('curl.exe https://example.com')).toBe('curl');
  });

  it('leaves an endpoint alone', () => {
    expect(importable('helloworld.Greeter/SayHello')).toBe(null);
    expect(importable('GET /v1/users')).toBe(null);
    expect(importable('grpcurl')).toBe(null);
    expect(importable('')).toBe(null);
  });

  it('leaves another program alone', () => {
    expect(importable('http https://example.com')).toBe(null);
    expect(importable('grpcurl-wrapper -plaintext host:1 a.B/C')).toBe(null);
  });

  it('reads the workbench own command', () => {
    expect(importable("grpctestify call -e 'a.B/C' --plaintext")).toBe('grpctestify');
    expect(importable('/usr/local/bin/grpctestify run tests/')).toBe('grpctestify');
    expect(importable('grpctestify')).toBeNull();
  });
});
