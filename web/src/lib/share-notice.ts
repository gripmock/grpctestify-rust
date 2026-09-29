const SIGNS: { test: RegExp; reason: string }[] = [
  { test: /\beyJ[A-Za-z0-9_-]{10,}\./, reason: 'looks like a JWT' },
  { test: /\bBearer\s+[A-Za-z0-9._~+/-]{8,}/i, reason: 'looks like a bearer token' },
  { test: /"(?:password|passwd|secret|api[_-]?key|access[_-]?token|refresh[_-]?token|private[_-]?key)"\s*:/i,
    reason: 'has a field named like a credential' },
  { test: /-----BEGIN [A-Z ]*PRIVATE KEY-----/, reason: 'carries a private key' },
];

export function credentialLooking(text: string): string | null {
  for (const sign of SIGNS) {
    if (sign.test.test(text)) return sign.reason;
  }
  return null;
}

export function bodyWarnings(bodies: string[]): { index: number; reason: string }[] {
  return bodies
    .map((body, index) => ({ index, reason: credentialLooking(body ?? '') }))
    .filter((w): w is { index: number; reason: string } => w.reason !== null);
}
