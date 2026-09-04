export function revealDelta(top: number, height: number, viewportHeight: number, share = 0.55): number {
  const want = Math.min(height, Math.round(viewportHeight * share));
  const visible = Math.min(viewportHeight, top + height) - Math.max(0, top);
  if (visible >= want) return 0;
  return Math.round(top - (viewportHeight - want));
}
