import { describe, it, expect } from 'vitest';
import { shapeOfMethod, shapeOfRequest, shapeMismatch, shapeSource, SHAPE_TONE, stepShape } from './shape';
import type { ReflectionMethod } from './types';

function m(fullName: string, clientStreaming: boolean, serverStreaming: boolean): ReflectionMethod {
  return { name: fullName.split('/').pop()!, fullName, service: fullName.split('/')[0], clientStreaming, serverStreaming };
}

const METHODS = [
  m('pkg.Svc/Unary', false, false),
  m('pkg.Svc/Down', false, true),
  m('pkg.Svc/Up', true, false),
  m('pkg.Svc/Both', true, true),
];

describe('shapeOfMethod', () => {
  it('maps all four shapes', () => {
    expect(shapeOfMethod(METHODS[0])).toBe('unary');
    expect(shapeOfMethod(METHODS[1])).toBe('server');
    expect(shapeOfMethod(METHODS[2])).toBe('client');
    expect(shapeOfMethod(METHODS[3])).toBe('bidi');
  });
});

describe('shapeOfRequest', () => {
  it('prefers the reflected method over the body count', () => {
    expect(shapeOfRequest('pkg.Svc/Down', 3, METHODS)).toBe('server');
  });

  it('falls back to client streaming when several bodies and no schema', () => {
    expect(shapeOfRequest('unknown.Svc/M', 2, METHODS)).toBe('client');
  });

  it('falls back to unary for a single body and no schema', () => {
    expect(shapeOfRequest('unknown.Svc/M', 1, METHODS)).toBe('unary');
    expect(shapeOfRequest('', 1, [])).toBe('unary');
  });
});

describe('shapeMismatch', () => {
  it('flags several bodies against a non-client-streaming method', () => {
    expect(shapeMismatch('pkg.Svc/Unary', 2, METHODS)).toBe(true);
  });

  it('is quiet when the method accepts a stream, or when the schema is unknown', () => {
    expect(shapeMismatch('pkg.Svc/Up', 2, METHODS)).toBe(false);
    expect(shapeMismatch('unknown.Svc/M', 5, METHODS)).toBe(false);
  });
});

describe('SHAPE_TONE', () => {
  it('names hues by shape, not by protocol', () => {
    expect(Object.values(SHAPE_TONE)).toEqual(['kind-simple', 'kind-down', 'kind-up', 'kind-duplex']);
  });
});

describe('what the call revealed', () => {
  it('reads a stream from the messages that came back', () => {
    expect(shapeOfRequest('unknown.Svc/M', 1, [], 3)).toBe('server');
  });

  it('reads both directions when both streamed', () => {
    expect(shapeOfRequest('unknown.Svc/M', 2, [], 3)).toBe('bidi');
  });

  it('lets the schema win over the observation', () => {
    expect(shapeOfRequest('pkg.Svc/Unary', 1, METHODS, 3)).toBe('unary');
  });
});

describe('what the last call reported', () => {
  it('beats a count-based guess when the schema is not loaded', () => {
    expect(shapeOfRequest('unknown.Svc/M', 3, [], 0, 'client')).toBe('client');
    expect(shapeOfRequest('unknown.Svc/M', 1, [], 3, 'unary')).toBe('unary');
    expect(shapeOfRequest('unknown.Svc/M', 1, [], 0, 'duplex')).toBe('bidi');
  });

  it('loses to the schema, which is the authority', () => {
    expect(shapeOfRequest('pkg.Svc/Down', 1, METHODS, 0, 'client')).toBe('server');
  });

  it('is ignored when it is not a shape name', () => {
    expect(shapeOfRequest('unknown.Svc/M', 2, [], 0, null)).toBe('client');
    expect(shapeOfRequest('unknown.Svc/M', 2, [], 0, 'nonsense')).toBe('client');
  });

  it('lets several messages to a reported-unary method read as a mismatch', () => {
    expect(shapeMismatch('unknown.Svc/M', 3, [], 'unary')).toBe(true);
    expect(shapeMismatch('unknown.Svc/M', 3, [], 'client')).toBe(false);
    expect(shapeMismatch('unknown.Svc/M', 3, [], null)).toBe(false);
  });
});

describe('shapeSource', () => {
  it('names the schema when the method is in it', () => {
    expect(shapeSource('pkg.Svc/Down', METHODS, null)).toBe('schema');
  });

  it('names the call when only the call knows', () => {
    expect(shapeSource('unknown.Svc/M', [], 'client')).toBe('call');
  });

  it('admits a guess when nothing knows', () => {
    expect(shapeSource('unknown.Svc/M', [], null)).toBe('guess');
    expect(shapeSource('unknown.Svc/M', [], 'nonsense')).toBe('guess');
  });
});

describe('what one step of a chain is', () => {
  it('says which transport an HTTP step is', () => {
    expect(stepShape({ kind: 'unary', endpoint: 'GET /api/health' }))
      .toEqual({ label: 'http', tone: 'kind-down' });
  });

  it('keeps the shape of a gRPC call', () => {
    expect(stepShape({ kind: 'server', endpoint: 'a.A/Watch' }))
      .toEqual({ label: 'server', tone: 'kind-down' });
    expect(stepShape({ kind: 'unary', endpoint: 'a.A/One' }).label).toBe('unary');
    expect(stepShape({ kind: 'bidi', endpoint: 'a.A/Chat' }).label).toBe('bidi');
  });

  it('reads the step, not the file it sits in', () => {
    expect(stepShape({ kind: 'unary', endpoint: 'POST /v1/users' }).label).toBe('http');
    expect(stepShape({ kind: 'unary', endpoint: 'helloworld.Greeter/SayHello' }).label).toBe('unary');
  });
});
