const HTTP = [
  'accept',
  'accept-encoding',
  'accept-language',
  'authorization',
  'cache-control',
  'content-type',
  'cookie',
  'if-match',
  'if-none-match',
  'origin',
  'referer',
  'user-agent',
  'x-api-key',
  'x-correlation-id',
  'x-request-id',
];

const GRPC = [
  'authorization',
  'grpc-timeout',
  'x-api-key',
  'x-correlation-id',
  'x-request-id',
  'x-tenant-id',
  'x-trace-id',
];

export function knownHeaders(wire: 'http' | 'grpc'): string[] {
  return wire === 'http' ? HTTP : GRPC;
}
