// Pure deep-link URL helpers — no store/React imports so they stay unit-testable
// without a DOM (localStorage) environment.

export type DeepLink =
  | { kind: 'collection'; value: string }
  | { kind: 'share'; value: string };

/** Parse a deep-link pathname. Values are URI-decoded (see encodeCollectionLink). */
export function parseDeepLink(pathname: string): DeepLink | null {
  const cMatch = pathname.match(/^\/c\/(.+)/);
  if (cMatch) return { kind: 'collection', value: decodeURIComponent(cMatch[1]) };
  const sMatch = pathname.match(/^\/s\/(.+)/);
  if (sMatch) return { kind: 'share', value: decodeURIComponent(sMatch[1]) };
  return null;
}

/** Build a collection deep-link path (inverse of parseDeepLink). */
export function encodeCollectionLink(collectionPath: string): string {
  return `/c/${encodeURIComponent(collectionPath)}`;
}
