import type { SectionAttribute } from './types';

export interface Overruled {
  section: string;
  value: string;
}

const ALIASES: Record<string, string[]> = {
  timeout: ['timeout'],
  retry: ['retry'],
  retry_delay: ['retry_delay', 'retry-delay'],
  no_retry: ['no_retry', 'no-retry'],
  compression: ['compression'],
};

export function overruledBy(attributes: SectionAttribute[], key: string): Overruled | null {
  const names = ALIASES[key] ?? [key];
  const found = attributes.find(a => names.includes(a.name));
  return found ? { section: found.section, value: found.value } : null;
}

export function delayUnused(options: Record<string, string>, attributes: SectionAttribute[]): boolean {
  if ((options.retry_delay ?? options['retry-delay'] ?? '').trim() === '') return false;
  const attempts = overruledBy(attributes, 'retry')?.value ?? options.retry ?? '';
  const noRetry = overruledBy(attributes, 'no_retry')?.value
    ?? options.no_retry ?? options['no-retry'] ?? '';
  const off = ['true', '1', 'yes', 'on'].includes(noRetry.trim().toLowerCase());
  return off || Number(attempts.trim() || '0') <= 0;
}
