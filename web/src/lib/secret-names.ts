const SECRET_WORDS = [
  'token', 'secret', 'password', 'passwd', 'apikey', 'api_key', 'api-key',
  'credential', 'private_key', 'private-key', 'privatekey', 'auth',
];

export function looksLikeSecret(name: string, named?: readonly string[]): boolean {
  const lower = name.trim().toLowerCase();
  if (lower === '') return false;
  if (named?.some(n => n.trim().toLowerCase() === lower)) return true;
  return SECRET_WORDS.some(word => lower.includes(word));
}

export const MASK = '••••••';

export function maskValue(name: string, value: string | undefined, named?: readonly string[]): string {
  if (value === undefined || value === '') return '';
  return looksLikeSecret(name, named) ? MASK : value;
}
