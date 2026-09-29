import { describe, expect, it } from 'vitest';
import { parseShell } from './shell';
import { callSummary, grpctestifySubcommand, isGrpctestify, parseGrpctestifyCall } from './gctf-call-import';

const parse = (line: string) => parseGrpctestifyCall(parseShell(line));

describe('the line the workbench writes', () => {
  it('is recognised however the binary is named', () => {
    expect(isGrpctestify('grpctestify call -e a/B')).toBe(true);
    expect(isGrpctestify('$ /usr/local/bin/grpctestify call -e a/B')).toBe(true);
    expect(isGrpctestify('grpcurl -plaintext host a/B')).toBe(false);
  });

  it('names the subcommand, and only the first word that is not a flag', () => {
    expect(grpctestifySubcommand(parseShell('grpctestify call -e a/B'))).toBe('call');
    expect(grpctestifySubcommand(parseShell('grpctestify -v run tests/'))).toBe('run');
    expect(grpctestifySubcommand(parseShell('grpctestify'))).toBe('');
  });

  it('reads back the request the copy button wrote', () => {
    const call = parse(
      "grpctestify call -e 'echo.EchoService/SayHello' --address 'localhost:50051'"
      + " -d '{\"message\":\"World\"}' --plaintext -H 'authorization: Bearer x'",
    );
    expect(call.endpoint).toBe('echo.EchoService/SayHello');
    expect(call.address).toBe('localhost:50051');
    expect(call.body).toBe('{"message":"World"}');
    expect(call.plaintext).toBe(true);
    expect(call.headers).toEqual({ authorization: 'Bearer x' });
    expect(call.ignored).toEqual([]);
  });

  it('reads the file form as the file, not as an endpoint', () => {
    const call = parse("grpctestify call 'api/hello.gctf' --doc-index 2");
    expect(call.file).toBe('api/hello.gctf');
    expect(call.endpoint).toBe('');
    expect(call.docIndex).toBe(2);
  });

  it('fills the sections the flags belong to', () => {
    const call = parse('grpctestify call -e a/B --protocol connectrpc --tls-ca ca.pem --max-time 5');
    expect(call.protocol).toBe('connectrpc');
    expect(call.tls).toEqual({ ca_cert: 'ca.pem' });
    expect(call.options).toEqual({ 'max-time': '5' });
    expect(callSummary(call)).toEqual(['TLS', 'connectrpc']);
  });

  it('names what a request cannot carry instead of dropping it', () => {
    const call = parse('grpctestify call -e a/B -v -o out.json --bench --requests 100');
    expect(call.ignored).toEqual(['--bench', '--requests 100', '-o out.json', '-v']);
    expect(call.endpoint).toBe('a/B');
  });

  it('does not read a malformed header as a header', () => {
    const call = parse("grpctestify call -e a/B -H 'authorization'");
    expect(call.headers).toEqual({});
    expect(call.ignored).toEqual(['-H authorization']);
  });
});
