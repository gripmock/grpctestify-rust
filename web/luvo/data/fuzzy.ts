/** Ranking the palette's file matches.
 *
 *  Matching was a subsequence test with no score and no order: `log` matched
 *  `auth/login.gctf` and `catalog/items/create.gctf` alike — c-a-t-a-**l**-**o**-**g** —
 *  and the list came back in whatever order the directory did. In a suite of a
 *  hundred files the answer you meant sits under a page of accidents. */

/** Higher is better; `null` is no match at all. */
export function scorePath(path: string, query: string): number | null {
  const q = query.trim().toLowerCase();
  if (q === '') return 0;
  const hay = path.toLowerCase();
  const name = hay.slice(hay.lastIndexOf('/') + 1);

  /* What was typed, as typed, in the file's own name — the case where there is
     nothing to guess. Earlier in the name is better, and a name that is only
     what was typed is the best of all. */
  const inName = name.indexOf(q);
  if (inName !== -1) return 1000 - inName * 10 - (name.length - q.length);

  const inPath = hay.indexOf(q);
  if (inPath !== -1) return 500 - inPath;

  /* Every letter in order, anywhere: kept, because `cip` for
     `catalog/items/pricing` is a real way to type — but always below a
     substring, and scored by how tightly the letters sit together. */
  let at = 0;
  let first = -1;
  let last = -1;
  for (const ch of q) {
    if (ch === ' ') continue;
    at = hay.indexOf(ch, at);
    if (at === -1) return null;
    if (first === -1) first = at;
    last = at;
    at++;
  }
  const spread = last - first;
  return 100 - Math.min(spread, 90);
}

/** The matches, best first. Ties keep the order they came in, so a list does
 *  not reshuffle under the cursor between two keystrokes. */
export function rankPaths(paths: string[], query: string): string[] {
  return paths
    .map((path, i) => ({ path, i, score: scorePath(path, query) }))
    .filter((m): m is { path: string; i: number; score: number } => m.score !== null)
    .sort((a, b) => b.score - a.score || a.i - b.i)
    .map(m => m.path);
}
