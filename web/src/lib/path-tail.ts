export function pathTail(path: string, max = 48): string {
  const whole = path.trim();
  if (whole.length <= max) return whole;

  const parts = whole.split('/').filter(Boolean);
  let kept = parts.pop() ?? whole;
  while (parts.length > 0) {
    const wider = `${parts[parts.length - 1]}/${kept}`;
    if (wider.length + 2 > max) break;
    kept = wider;
    parts.pop();
  }
  return `…/${kept}`;
}
