import { describe, expect, it } from 'vitest';
import { schemaMiss, servicesOf } from './schema-miss';

describe('a service the target does not serve', () => {
  it('says which target refused it', () => {
    const miss = schemaMiss({
      reason: "Service 'auth.v1.AuthService' not found",
      address: 'localhost:4770',
      services: ['helloworld.Greeter', 'grpc.health.v1.Health'],
    });
    expect(miss?.title).toBe('auth.v1.AuthService is not on localhost:4770.');
    expect(miss?.services).toEqual(['grpc.health.v1.Health', 'helloworld.Greeter']);
  });

  it('never lists the service that is missing', () => {
    const miss = schemaMiss({
      reason: "Service 'a.B' not found",
      address: 'localhost:1',
      services: ['a.B', 'c.D'],
    });
    expect(miss?.services).toEqual(['c.D']);
  });

  it('still says something with no address to name', () => {
    expect(schemaMiss({ reason: "Service 'a.B' not found", address: '  ', services: [] })?.title)
      .toBe('a.B is not on this target.');
  });

  it('leaves a failure it does not recognise as it is', () => {
    expect(schemaMiss({ reason: 'connection refused', address: 'x', services: [] })).toBeNull();
    expect(schemaMiss({ reason: 'The target returned no schema', address: 'x', services: [] })).toBeNull();
  });
});

describe('the services behind a method list', () => {
  it('names each one once, in order', () => {
    expect(servicesOf([
      { service: 'Two', fullName: 'b.Two/M' },
      { service: 'One', fullName: 'a.One/M' },
      { service: 'Two', fullName: 'b.Two/N' },
    ])).toEqual(['a.One', 'b.Two']);
  });

  it('names them the way the file does', () => {
    expect(servicesOf([{ service: 'Greeter', fullName: 'helloworld.Greeter/SayHello' }]))
      .toEqual(['helloworld.Greeter']);
  });

  it('falls back to the short name when there is nothing fuller', () => {
    expect(servicesOf([{ service: 'a.One' }])).toEqual(['a.One']);
  });

  it('has nothing to say about methods that name none', () => {
    expect(servicesOf([{ service: '' }, { service: '  ' }])).toEqual([]);
  });
});
