export type ProtoKind = 'proto' | 'descriptor';

export interface ProtoFile {
  path: string;
  name: string;
  size: number;
  kind: ProtoKind;
}

export function protoKindOf(filename: string): ProtoKind | null {
  const name = filename.toLowerCase();
  if (name.endsWith('.proto')) return 'proto';
  if (['.pb', '.bin', '.desc', '.protoset'].some(ext => name.endsWith(ext))) return 'descriptor';
  return null;
}

export function bytesToBase64(bytes: Uint8Array): string {
  let binary = '';
  const CHUNK = 0x8000;
  for (let i = 0; i < bytes.length; i += CHUNK) {
    binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return btoa(binary);
}

export function refusalFor(filename: string): string {
  return `${filename}: only .gctf, .httf, .proto and a descriptor set (.pb, .bin, .desc, .protoset) can be dropped here`;
}
