export type DeepLink =
  | { kind: 'collection'; value: string }
  | { kind: 'share'; value: string };

export function parseDeepLink(pathname: string): DeepLink | null {
  const cMatch = pathname.match(/^\/c\/(.+)/);
  if (cMatch) return { kind: 'collection', value: decodeURIComponent(cMatch[1]) };
  const sMatch = pathname.match(/^\/s\/(.+)/);
  if (sMatch) return { kind: 'share', value: decodeURIComponent(sMatch[1]) };
  return null;
}

export function encodeCollectionLink(collectionPath: string): string {
  return `/c/${encodeURIComponent(collectionPath)}`;
}

export function urlWhenLinkFails(workspacePath: string | null): string {
  return workspacePath ? encodeCollectionLink(workspacePath) : '/';
}

export function nextUrl(current: string, workspacePath: string | null, pendingCollection: string | null): string | null {
  if (current.startsWith('/s/')) return null;
  if (pendingCollection !== null && pendingCollection !== workspacePath) return null;
  const next = workspacePath ? encodeCollectionLink(workspacePath) : '/';
  return next === current ? null : next;
}
