export type TokenKind = 'key' | 'str' | 'num' | 'punct' | 'plain';
export interface Token {
  kind: TokenKind;
  text: string;
}

const PATTERN = /("(?:[^"\\]|\\.)*"\s*:)|("(?:[^"\\]|\\.)*")|(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?|true|false|null)|([{}[\],:])/g;

/** JSON as tokens, for the same `.tok-*` colours the response pane uses.
 *
 *  A tokenizer rather than a highlighter library: the documentation shows JSON
 *  and gRPC status lines, and the workbench already has the four colours. */
export function tokenizeJson(text: string): Token[] {
  const out: Token[] = [];
  let at = 0;
  for (const m of text.matchAll(PATTERN)) {
    const index = m.index ?? 0;
    if (index > at) out.push({ kind: 'plain', text: text.slice(at, index) });
    const [all, key, str, literal, punct] = m;
    if (key !== undefined) {
      /* `"name":` is a key and a colon: the colon is punctuation, and colouring
         it as part of the key made every key end in a coloured comma-like tail. */
      const colon = all.lastIndexOf(':');
      out.push({ kind: 'key', text: all.slice(0, colon) });
      out.push({ kind: 'punct', text: all.slice(colon) });
    } else if (str !== undefined) {
      out.push({ kind: 'str', text: str });
    } else if (literal !== undefined) {
      out.push({ kind: 'num', text: literal });
    } else if (punct !== undefined) {
      out.push({ kind: 'punct', text: punct });
    }
    at = index + all.length;
  }
  if (at < text.length) out.push({ kind: 'plain', text: text.slice(at) });
  return out;
}
