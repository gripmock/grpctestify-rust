import { describe, expect, it } from 'vitest';
import { callKindOf, switchCall, switchable } from './call-kind';

describe('what a request is', () => {
  it('is the file family where a file says so', () => {
    expect(callKindOf('a/login.gctf', 'GET /v1/users')).toBe('grpc');
    expect(callKindOf('a/list.httf', 'a.A/One')).toBe('http');
  });

  it('is the endpoint where the file does not', () => {
    expect(callKindOf(null, 'GET /v1/users')).toBe('http');
    expect(callKindOf('c.apif', 'a.A/One')).toBe('grpc');
    expect(callKindOf(null, '')).toBe('grpc');
  });
});

describe('whether it may be the other thing', () => {
  it('is fixed by a saved file’s extension, and says which way out', () => {
    expect(switchable('a/login.gctf')).toEqual({
      can: false,
      why: 'A .gctf is a gRPC test — save it as a .httf to make an HTTP one',
    });
    expect(switchable('a/list.httf').can).toBe(false);
  });

  it('is open for an untitled tab and for a file that holds both', () => {
    expect(switchable(null).can).toBe(true);
    expect(switchable('checkout.apif').can).toBe(true);
  });
});

describe('switching what a request is', () => {
  const grpcDefault = 'localhost:4770';

  it('starts the other kind from its own grammar', () => {
    const said = switchCall({
      to: 'http',
      endpoint: 'a.A/One',
      other: '',
      address: grpcDefault,
      addressTouched: false,
      grpcDefault,
    });
    expect(said.endpoint).toBe('GET /');
    expect(said.other).toBe('a.A/One');
  });

  it('brings back what the other kind held', () => {
    const said = switchCall({
      to: 'grpc',
      endpoint: 'GET /v1/users',
      other: 'a.A/One',
      address: '',
      addressTouched: false,
      grpcDefault,
    });
    expect(said.endpoint).toBe('a.A/One');
    expect(said.other).toBe('GET /v1/users');
  });

  it('takes the default address across only where it makes sense', () => {
    expect(switchCall({
      to: 'http', endpoint: 'a.A/One', other: '', address: grpcDefault, addressTouched: false, grpcDefault,
    }).address).toBe('');
    expect(switchCall({
      to: 'grpc', endpoint: 'GET /a', other: '', address: '', addressTouched: false, grpcDefault,
    }).address).toBe(grpcDefault);
  });

  it('never touches an address someone typed', () => {
    expect(switchCall({
      to: 'http', endpoint: 'a.A/One', other: '', address: 'localhost:4770', addressTouched: true, grpcDefault,
    }).address).toBe('localhost:4770');
    expect(switchCall({
      to: 'grpc', endpoint: 'GET /a', other: '', address: 'https://api.test', addressTouched: true, grpcDefault,
    }).address).toBe('https://api.test');
  });
});
