import { looksHttp } from './http-endpoint';

export type CallKind = 'grpc' | 'http';

export function callKindOf(path: string | null | undefined, endpoint: string): CallKind {
  const name = path ?? '';
  if (name.endsWith('.httf')) return 'http';
  if (name.endsWith('.gctf')) return 'grpc';
  return looksHttp(endpoint) ? 'http' : 'grpc';
}

export function switchable(path: string | null | undefined): { can: boolean; why: string } {
  const name = path ?? '';
  if (name.endsWith('.gctf')) {
    return { can: false, why: 'A .gctf is a gRPC test — save it as a .httf to make an HTTP one' };
  }
  if (name.endsWith('.httf')) {
    return { can: false, why: 'A .httf is an HTTP test — save it as a .gctf to make a gRPC one' };
  }
  return { can: true, why: '' };
}

export interface CallSwitch {
  endpoint: string;
  other: string;
  address: string;
}

export function switchCall(input: {
  to: CallKind;
  endpoint: string;
  other: string;
  address: string;
  addressTouched: boolean;
  grpcDefault: string;
}): CallSwitch {
  const endpoint = input.other.trim() !== ''
    ? input.other
    : input.to === 'http' ? 'GET /' : '';

  const address = input.addressTouched
    ? input.address
    : input.to === 'http'
      ? (input.address.trim() === input.grpcDefault ? '' : input.address)
      : (input.address.trim() === '' ? input.grpcDefault : input.address);

  return { endpoint, other: input.endpoint, address };
}
