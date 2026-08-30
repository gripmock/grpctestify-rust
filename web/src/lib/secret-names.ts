const SECRET_WORDS = [
  'token', 'secret', 'password', 'passwd', 'apikey', 'api_key', 'api-key',
  'credential', 'private_key', 'private-key', 'privatekey', 'auth',
];

export function looksLikeSecret(name: string): boolean {
  const lower = name.trim().toLowerCase();
  if (lower === '') return false;
  return SECRET_WORDS.some(word => lower.includes(word));
}

export function maskValue(name: string, value: string | undefined): string {
  if (value === undefined || value === '') return '';
  return looksLikeSecret(name) ? '••••••' : value;
}
