export function statusUnchecked(asserts: string[]): boolean {
  return !asserts.some(a => a.includes('@status'));
}

export function statusAssert(code: number | null | undefined): string {
  const known = typeof code === 'number' && code >= 100 && code < 600;
  return `@status() == ${known ? code : 200}`;
}
