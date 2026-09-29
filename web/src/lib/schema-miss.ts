export interface SchemaMiss {
  title: string;
  services: string[];
}

const NOT_FOUND = /^Service '([^']+)' not found/;

export function schemaMiss(input: {
  reason: string;
  address: string;
  services: string[];
}): SchemaMiss | null {
  const missing = NOT_FOUND.exec(input.reason.trim());
  if (!missing) return null;

  const where = input.address.trim();
  return {
    title: where === ''
      ? `${missing[1]} is not on this target.`
      : `${missing[1]} is not on ${where}.`,
    services: [...new Set(input.services)].filter(s => s !== missing[1]).sort(),
  };
}

export function servicesOf(methods: { service: string; fullName?: string }[]): string[] {
  const named = methods
    .map(m => (m.fullName?.split('/')[0] || m.service).trim())
    .filter(name => name !== '');
  return [...new Set(named)].sort();
}
