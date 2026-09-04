export interface MetadataNote {
  level: 'note' | 'bad';
  reason: string;
}

export type HeaderWire = 'grpc' | 'http';

export function checkMetadataKey(key: string, wire: HeaderWire = 'grpc'): MetadataNote | null {
  if (key === '') return { level: 'bad', reason: 'This header has no name — it will not be sent' };
  if (key.startsWith(':')) return { level: 'bad', reason: 'Keys starting with ":" are reserved by HTTP/2' };
  if (/[^0-9A-Za-z_.-]/.test(key)) {
    return { level: 'bad', reason: 'A header name holds letters, digits, "-", "_" and "."' };
  }
  if (wire === 'grpc' && /[A-Z]/.test(key)) {
    return { level: 'note', reason: `gRPC lowercases keys — this travels as ${key.toLowerCase()}` };
  }
  if (wire === 'http' && key.toLowerCase() === 'content-length') {
    return { level: 'bad', reason: 'The call sets this from the body it sends — this one is dropped' };
  }
  return null;
}

export function isBase64(value: string): boolean {
  const body = value.replace(/=+$/, '');
  if (!/^[A-Za-z0-9+/]*$/.test(body)) return false;
  return body.length % 4 !== 1;
}

export function checkMetadataValue(key: string, value: string, wire: HeaderWire = 'grpc'): MetadataNote | null {
  if (value === '') return null;
  if (value.includes('{{')) return null;
  if (wire === 'http') {
    return /[^\x20-\x7e]/.test(value)
      ? { level: 'bad', reason: 'A header value travels as printable ASCII — this one does not' }
      : null;
  }
  if (key.toLowerCase().endsWith('-bin')) {
    return isBase64(value)
      ? null
      : { level: 'bad', reason: 'A "-bin" key carries base64; this value is not base64' };
  }
  if (/[^\x20-\x7e]/.test(value)) {
    return { level: 'bad', reason: 'Only printable ASCII travels in a text header — use a "-bin" key and base64' };
  }
  return null;
}
