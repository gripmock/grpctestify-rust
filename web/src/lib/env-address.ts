import { checkAddress } from './address';
export function aimsHttp(address: string): boolean {
  return address.includes('://');
}

export function targetNote(address: string): string | null {
  const value = address.trim();
  if (value === '' || aimsHttp(value)) return null;
  return 'gRPC files dial this as written; an HTTP file needs a scheme — `http://` or `https://` in front of it aims both.';
}

export function environmentAddressNote(address: string): { said: string; bad: boolean } | null {
  const verdict = checkAddress(address, 'httf');
  if (!verdict.ok && verdict.reason !== undefined) return { said: verdict.reason, bad: true };
  if (verdict.ok && verdict.note !== undefined) return { said: verdict.note, bad: false };
  return null;
}
