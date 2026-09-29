export function filterProto(source: string, query: string): string {
  const q = query.trim().toLowerCase();
  if (q === '') return source;

  const lines = source.split('\n');
  const kept: string[] = [];
  let header: string | null = null;
  let inner: string[] = [];
  let wholeBlock = false;

  const flush = () => {
    if (header === null) return;
    if (wholeBlock || inner.length > 0) {
      if (kept.length > 0) kept.push('');
      kept.push(header, ...inner, '}');
    }
    header = null;
    inner = [];
    wholeBlock = false;
  };

  for (const line of lines) {
    const hit = line.toLowerCase().includes(q);
    if (header === null) {
      if (line.endsWith('{')) {
        header = line;
        wholeBlock = hit;
      }
      continue;
    }
    if (line === '}') {
      flush();
      continue;
    }
    if (wholeBlock || hit) inner.push(line);
  }
  flush();

  return kept.join('\n');
}
