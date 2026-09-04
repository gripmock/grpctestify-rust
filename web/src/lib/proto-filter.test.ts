import { describe, expect, it } from 'vitest';
import { filterProto } from './proto-filter';

const SOURCE = [
  'service echo.EchoService {',
  '  rpc SayHello(HelloRequest) returns (HelloResponse);',
  '  rpc Echo(EchoRequest) returns (EchoResponse);',
  '}',
  '',
  'message HelloRequest {',
  '  string message = 1;',
  '}',
  '',
  'message EchoRequest {',
  '  string text = 1;',
  '  int32 count = 2;',
  '}',
].join('\n');

describe('filtering a schema', () => {
  it('keeps a match under the message it belongs to', () => {
    expect(filterProto(SOURCE, 'count')).toBe(
      ['message EchoRequest {', '  int32 count = 2;', '}'].join('\n'),
    );
  });

  it('keeps the whole block when the block is what matched', () => {
    expect(filterProto(SOURCE, 'HelloRequest')).toBe(
      [
        'service echo.EchoService {',
        '  rpc SayHello(HelloRequest) returns (HelloResponse);',
        '}',
        '',
        'message HelloRequest {',
        '  string message = 1;',
        '}',
      ].join('\n'),
    );
  });

  it('is the whole schema when nothing is asked', () => {
    expect(filterProto(SOURCE, '   ')).toBe(SOURCE);
  });

  it('is nothing when nothing matches', () => {
    expect(filterProto(SOURCE, 'zzz')).toBe('');
  });
});
