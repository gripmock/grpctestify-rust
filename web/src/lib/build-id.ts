export function loadedBuild(doc: Document = document): string | null {
  const script = doc.querySelector<HTMLScriptElement>('script[type="module"][src]');
  if (!script) return null;
  const src = script.getAttribute('src') ?? '';
  const path = src.startsWith('http') ? new URL(src).pathname : src;
  return path.replace(/^\//, '') || null;
}

export function buildMoved(loaded: string | null, served: string | null | undefined): boolean {
  if (!loaded || !served) return false;
  return loaded !== served;
}

export function isStaleChunkError(message: string): boolean {
  return /dynamically imported module|Importing a module script failed|error loading dynamically imported module/i
    .test(message);
}
