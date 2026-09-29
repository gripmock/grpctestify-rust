import { describe, it, expect } from 'vitest';
import { defaultAddressFor, dialledAddress } from './types';

describe('defaultAddressFor', () => {
  it('grpc → 4770', () => {
    expect(defaultAddressFor('grpc')).toBe('localhost:4770');
  });

  it('grpc-web → 4769', () => {
    expect(defaultAddressFor('grpc-web')).toBe('localhost:4769');
  });

  it('connectrpc → 4769', () => {
    expect(defaultAddressFor('connectrpc')).toBe('localhost:4769');
  });
});

describe('dialledAddress', () => {
  it('is what was typed', () => {
    expect(dialledAddress('example.com:9000', 'grpc')).toBe('example.com:9000');
    expect(dialledAddress('  example.com:9000  ', 'grpc-web')).toBe('example.com:9000');
  });

  it('is the transport default when the field is empty', () => {
    expect(dialledAddress('', 'grpc')).toBe('localhost:4770');
    expect(dialledAddress('   ', 'grpc-web')).toBe('localhost:4769');
    expect(dialledAddress('', 'connectrpc')).toBe('localhost:4769');
  });
});
