import { describe, it, expect } from 'vitest';
import { explainFailure, unresolvedNames } from './failure';

describe('explainFailure', () => {
  it('names an unreachable port and what to do about it', () => {
    const f = explainFailure('Could not reach localhost:59999: Connection refused (os error 61)');
    expect(f.title).toBe('Could not reach localhost:59999');
    expect(f.detail).toContain('Connection refused');
    expect(f.fixes[0]).toContain('Nothing is listening');
  });

  it('tells an unknown host from a refused one', () => {
    const f = explainFailure('Could not reach nope.invalid:4770: failed to lookup address information: nodename nor servname provided');
    expect(f.fixes[0]).toContain('host is unknown');
  });

  it('reads a non-gRPC port for what it is', () => {
    const f = explainFailure('Could not reach localhost:4771: connection error detected: frame with invalid size');
    expect(f.fixes[0]).toContain('not as gRPC');
    expect(f.fixes[1]).toContain('gRPC-Web');
  });

  it('offers the descriptor when the server serves no reflection', () => {
    const f = explainFailure('The server at localhost:4770 does not serve reflection. Start it with the reflection service, or name `PROTO descriptor:` in the file');
    expect(f.fixes).toHaveLength(2);
    expect(f.fixes[1]).toContain('PROTO descriptor');
  });

  it('advises from the status code when the message is the server\'s own', () => {
    const f = explainFailure('gRPC error code=12 message=unknown method Foo', 12);
    expect(f.title).toBe('unknown method Foo');
    expect(f.fixes[0]).toContain('not on this server');
  });

  it('says nothing about codes an application uses for its own answers', () => {
    expect(explainFailure('gRPC error code=5 message=No matching stub found', 5).fixes).toEqual([]);
    expect(explainFailure('gRPC error code=7 message=nope', 7).fixes).toEqual([]);
  });

  it('says nothing it cannot back up', () => {
    const f = explainFailure('something nobody has seen before');
    expect(f).toEqual({ title: 'something nobody has seen before', detail: null, fixes: [] });
  });
});

describe('unresolvedNames', () => {
  it('names what the run refused to send', () => {
    expect(unresolvedNames('Unresolved variable placeholder(s) in REQUEST at line 8: {{USER}}, {{TOKEN}}'))
      .toEqual(['USER', 'TOKEN']);
  });

  it('names each one once', () => {
    expect(unresolvedNames('Unresolved variable placeholder(s) in REQUEST_HEADERS: {{A}}, {{A}}')).toEqual(['A']);
  });

  it('finds nothing in any other failure', () => {
    expect(unresolvedNames('gRPC error code=14 message=connection refused')).toEqual([]);
    expect(unresolvedNames('expected {{USER}} in the body')).toEqual([]);
  });
});

describe('a port that answered with something else', () => {
  it('says so, instead of handing over the transport’s own words', () => {
    const failure = explainFailure(
      'Reflection failed at localhost:8871: Internal protocol error: received message with invalid compression flag: 73 (valid flags are 0 and 1) while receiving response with status: 403 Forbidden',
    );
    expect(failure.title).toBe('localhost:8871 answered, but not as gRPC');
    expect(failure.detail).toContain('invalid compression flag');
    expect(failure.fixes[0]).toContain('plain HTTP port');
    expect(failure.fixes.some(f => f.includes('.httf'))).toBe(true);
  });

  it('keeps the sentence when it cannot name the target', () => {
    expect(explainFailure('Internal protocol error: frame with invalid size').title)
      .toBe('The target answered, but not as gRPC');
  });

  it('leaves an unreachable address to the rule that already reads it', () => {
    expect(explainFailure('Could not reach localhost:59999: Connection refused (os error 61)').title)
      .toBe('Could not reach localhost:59999');
  });
});

describe('an address the words do not carry', () => {
  it('is the one the call dialled', () => {
    const failure = explainFailure('Internal protocol error: invalid compression flag: 73', null, 'localhost:8871');
    expect(failure.title).toBe('localhost:8871 answered, but not as gRPC');
  });

  it('never overrides an address the words do carry', () => {
    const failure = explainFailure('Reflection failed at api.example.com:443: http/1 response', null, 'localhost:1');
    expect(failure.title).toBe('api.example.com:443 answered, but not as gRPC');
  });
});

describe('a service or method the target does not have', () => {
  it('names the target that does not have it', () => {
    const f = explainFailure("Service 'auth.v1.AuthService' not found", 5, 'localhost:4770');
    expect(f.title).toBe('auth.v1.AuthService is not on localhost:4770');
  });

  it('says what that target does serve, when anything has asked', () => {
    const f = explainFailure("Service 'auth.v1.AuthService' not found", 5, 'localhost:4770', [
      'helloworld.Greeter',
      'grpc.health.v1.Health',
    ]);
    expect(f.fixes[0]).toBe('localhost:4770 serves grpc.health.v1.Health · helloworld.Greeter');
  });

  it('sends the reader to the field that asks, when nothing has', () => {
    const f = explainFailure("Service 'a.B' not found", 5, 'localhost:1');
    expect(f.fixes[0]).toContain('endpoint field');
  });

  it('tells a missing method from a missing service', () => {
    const method = explainFailure("Method 'Logout' not found", 5, 'localhost:4770');
    expect(method.title).toBe('Logout is not on localhost:4770');
    expect(method.fixes[1]).toContain('method name');

    const service = explainFailure("Service 'a.B' not found", 5, 'localhost:4770');
    expect(service.fixes[1]).toContain('address');
  });

  it('still says something with no address to name', () => {
    expect(explainFailure("Service 'a.B' not found", 5).title).toBe('a.B is not on this target');
  });
});

describe('a target that answered without speaking HTTP', () => {
  it('names the port for what it is', () => {
    const said = explainFailure(
      'http://127.0.0.1:4790/api/health did not answer: invalid HTTP version parsed',
    );
    expect(said.title).toBe('http://127.0.0.1:4790/api/health answered, but not as HTTP');
    expect(said.detail).toBe('invalid HTTP version parsed');
    expect(said.fixes[0]).toContain('gRPC port');
  });

  it('keeps a cause it does not recognise as the answer', () => {
    const said = explainFailure('http://api.test/v1 did not answer: body stream ended unexpectedly');
    expect(said.title).toBe('http://api.test/v1 did not answer');
    expect(said.detail).toBe('body stream ended unexpectedly');
    expect(said.fixes).toEqual([]);
  });

  it('reads a url as the target, not just a host and port', () => {
    const said = explainFailure('Could not reach http://127.0.0.1:4999: Connection refused (os error 61)');
    expect(said.title).toBe('Could not reach http://127.0.0.1:4999');
    expect(said.fixes[0]).toContain('Nothing is listening there');
  });
});
