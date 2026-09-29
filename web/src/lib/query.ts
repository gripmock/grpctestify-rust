export interface QueryParam {
  key: string;
  value: string;
  bare?: boolean;
}

export interface PathParts {
  path: string;
  params: QueryParam[];
}

export function splitPath(raw: string): PathParts {
  const at = raw.indexOf('?');
  if (at === -1) return { path: raw, params: [] };
  const path = raw.slice(0, at);
  const query = raw.slice(at + 1);
  if (query === '') return { path, params: [] };
  const params = query.split('&').map(pair => {
    const eq = pair.indexOf('=');
    return eq === -1
      ? { key: decodePart(pair), value: '', bare: true }
      : { key: decodePart(pair.slice(0, eq)), value: decodePart(pair.slice(eq + 1)), bare: false };
  });
  return { path, params };
}

export function splitForm(body: string): QueryParam[] {
  const trimmed = body.trim();
  if (trimmed === '') return [];
  return splitPath(`?${trimmed}`).params;
}

export function joinForm(params: QueryParam[]): string {
  return joinPath('', params).replace(/^\?/, '');
}

export function joinPath(path: string, params: QueryParam[]): string {
  const named = params.filter(p => p.key.trim() !== '');
  if (named.length === 0) return path;
  const query = named
    .map(p => (p.value === '' && p.bare !== false
      ? encodePart(p.key)
      : `${encodePart(p.key)}=${encodePart(p.value)}`))
    .join('&');
  return `${path}?${query}`;
}

function encodePart(raw: string): string {
  return raw
    .split(/(\{\{[^}]*\}\}|%7B|%7D)/gi)
    .map(piece => (/^(\{\{|%7B$|%7D$)/i.test(piece) ? piece : encodeURIComponent(piece)))
    .join('');
}

function decodePart(raw: string): string {
  try {
    return raw
      .split(/(%7B|%7D)/gi)
      .map((piece, i) => (i % 2 === 1 ? piece : decodeURIComponent(piece.replace(/\+/g, ' '))))
      .join('');
  } catch {
    return raw;
  }
}

export function emptyForm(param: QueryParam): string {
  return param.bare === false ? `${param.key}=` : param.key;
}
