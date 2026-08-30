const KEY = 'play.token';

export function tokenFromUrl(href: string): { token: string | null; url: string } {
  const at = href.indexOf('?');
  if (at === -1) return { token: null, url: href };
  const [base, query] = [href.slice(0, at), href.slice(at + 1)];
  const kept: string[] = [];
  let token: string | null = null;
  for (const pair of query.split('&')) {
    const [key, value = ''] = pair.split('=');
    if (key === 'token' && value !== '') token = decodeURIComponent(value);
    else if (pair !== '') kept.push(pair);
  }
  if (token === null) return { token: null, url: href };
  return { token, url: kept.length > 0 ? `${base}?${kept.join('&')}` : base };
}

export function claimToken(storage: Storage, location: { href: string }, replace: (url: string) => void): string | null {
  const { token, url } = tokenFromUrl(location.href);
  if (token !== null) {
    storage.setItem(KEY, token);
    replace(url);
    held = token;
    return token;
  }
  held = storage.getItem(KEY);
  return held;
}

let held: string | null = null;

export function hasToken(): boolean {
  return held !== null;
}

export function withToken(token: string | null, init?: RequestInit): RequestInit | undefined {
  if (token === null) return init;
  const headers = new Headers(init?.headers);
  headers.set('Authorization', `Bearer ${token}`);
  return { ...init, headers };
}

export function streamUrl(token: string | null, url: string): string {
  if (token === null) return url;
  return `${url}${url.includes('?') ? '&' : '?'}token=${encodeURIComponent(token)}`;
}

let rejected = false;
const listeners = new Set<() => void>();

export function noteUnauthorized(): void {
  if (rejected) return;
  rejected = true;
  for (const listener of listeners) listener();
}

export function tokenRejected(): boolean {
  return rejected;
}

export function subscribeUnauthorized(listener: () => void): () => void {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}
