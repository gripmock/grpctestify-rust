import { describe, it, expect } from 'vitest';
import { defaultAddressFor, isAddressAtDefault } from './types';

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

// Covers transport-architecture §9.7/§9.8: switching protocol updates the
// address to the new protocol's default only when the current address is
// still the old protocol's untouched default; a manually-entered address
// must survive a protocol switch unchanged.
describe('isAddressAtDefault', () => {
  it('true when address matches the protocol default', () => {
    expect(isAddressAtDefault('localhost:4770', 'grpc')).toBe(true);
    expect(isAddressAtDefault('localhost:4769', 'grpc-web')).toBe(true);
  });

  it('false for a manually overridden address', () => {
    expect(isAddressAtDefault('example.com:9000', 'grpc')).toBe(false);
  });

  it('false when address matches a different protocol\'s default', () => {
    // Still on grpc's default (4770) but checking against grpc-web's (4769).
    expect(isAddressAtDefault('localhost:4770', 'grpc-web')).toBe(false);
  });
});
