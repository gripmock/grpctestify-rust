import type { CollectionParsed, GctfMeta } from './types';

export function metaFromParsed(parsed: CollectionParsed | null): GctfMeta {
  return {
    name: parsed?.meta_name ?? undefined,
    summary: parsed?.meta_summary ?? undefined,
    owner: parsed?.meta_owner ?? undefined,
    tags: parsed?.meta_tags ?? [],
    links: parsed?.meta_links ?? [],
  };
}

export function addressForSave(
  parsed: Pick<CollectionParsed, 'address'> | null,
  typed: string,
  addressTouched: boolean,
): string | undefined {
  const value = typed.trim();
  if (!parsed) return value || undefined;
  if (addressTouched) return value || undefined;
  return parsed.address || undefined;
}

export function protocolForSave(
  parsed: Pick<CollectionParsed, 'options'> | null,
  protocol: string,
  protocolTouched: boolean,
): string | undefined {
  const own = parsed?.options?.protocol;
  const chosen = !parsed || protocolTouched ? protocol : own;
  return chosen && chosen !== 'grpc' ? chosen : undefined;
}
