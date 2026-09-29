/** WCAG contrast over the palette, computed from `tokens.css` without a browser.
 *
 * The token files are written in `rgb()` for exactly this reason: the grader
 * parses the declarations as text, so the floors can be enforced in a unit test
 * rather than eyeballed in a screenshot. */

export type Rgba = [number, number, number, number];

/** One `--name: value;` declaration per key, per selector block. */
export type TokenBlock = Record<string, string>;

const BLOCK = /((?::root|\[data-theme="[\w-]+"])[^{]*)\{([^}]*)\}/g;
const DECL = /(--[\w-]+)\s*:\s*([^;]+);/g;

/** Parse `tokens.css` into `{ selector: { token: value } }`. */
export function parseTokenBlocks(css: string): Record<string, TokenBlock> {
  const blocks: Record<string, TokenBlock> = {};
  for (const [, selector, body] of css.matchAll(BLOCK)) {
    // The same selector may appear more than once — the palette and the legacy
    // alias block are both `:root`. Merge in source order, as the cascade does.
    const key = selector.trim();
    const decls: TokenBlock = blocks[key] ?? {};
    for (const [, name, value] of body.matchAll(DECL)) decls[name] = value.trim();
    blocks[key] = decls;
  }
  return blocks;
}

/** A theme is the base block with its own layered over it, exactly as the
 *  cascade does it — `paper` is the base and has no block of its own. */
export function resolveTheme(css: string, id: string): TokenBlock {
  const blocks = parseTokenBlocks(css);
  const base = blocks[':root'] ?? {};
  return { ...base, ...(blocks[`[data-theme="${id}"]`] ?? {}) };
}

/** `rgb(1, 2, 3)`, `rgb(1 2 3 / 0.5)` and `var(--x)` chains. */
export function toRgba(value: string, tokens: TokenBlock, seen = new Set<string>()): Rgba | null {
  const v = value.trim();

  const ref = v.match(/^var\(\s*(--[\w-]+)/);
  if (ref) {
    const name = ref[1];
    if (seen.has(name)) return null;
    seen.add(name);
    const next = tokens[name];
    return next === undefined ? null : toRgba(next, tokens, seen);
  }

  const fn = v.match(/^rgba?\(([^)]+)\)$/);
  if (!fn) return null;
  const parts = fn[1].split(/[\s,/]+/).filter(Boolean).map(Number);
  if (parts.length < 3 || parts.slice(0, 3).some(Number.isNaN)) return null;
  const alpha = parts.length > 3 && !Number.isNaN(parts[3]) ? parts[3] : 1;
  return [parts[0], parts[1], parts[2], alpha];
}

function channel(c: number): number {
  const s = c / 255;
  return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
}

function luminance([r, g, b]: Rgba): number {
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}

/** A translucent foreground is composited over its background first, because
 *  that is what the eye sees — `--accent-soft` is 14% amber, not amber. */
export function contrast(fg: Rgba, bg: Rgba): number {
  const solid: Rgba = fg[3] < 1
    ? [0, 1, 2].map(i => fg[i] * fg[3] + bg[i] * (1 - fg[3])).concat([1]) as Rgba
    : fg;
  const a = luminance(solid);
  const b = luminance(bg);
  const hi = Math.max(a, b);
  const lo = Math.min(a, b);
  return Number(((hi + 0.05) / (lo + 0.05)).toFixed(2));
}

export function ratioOf(tokens: TokenBlock, fg: string, bg: string): number | null {
  const f = toRgba(tokens[fg] ?? '', tokens);
  const b = toRgba(tokens[bg] ?? '', tokens);
  return f && b ? contrast(f, b) : null;
}

/** The floors every mode has to clear.
 *
 * The accent is graded at 3 as a control rather than 4.5 as body text, so a
 * readable palette is not reported as marginal. `accent-ink` on `accent-fill`
 * is the filled primary button, which is where the old slate palette failed at
 * 3.68 and is why it is checked here at all. */
export const FLOORS: ReadonlyArray<readonly [string, string, number]> = [
  ['--ink', '--surface', 4.5],
  ['--ink', '--surface-raised', 4.5],
  ['--ink', '--surface-sunken', 4.5],
  ['--ink-muted', '--surface', 4.5],
  ['--ink-muted', '--surface-raised', 4.5],
  ['--ink-muted', '--surface-sunken', 4.5],
  ['--accent', '--surface', 3],
  ['--accent', '--surface-raised', 3],
  ['--accent-ink', '--accent-fill', 4],
  ['--ok', '--surface-raised', 3],
  ['--fail', '--surface-raised', 3],
  ['--warn', '--surface-raised', 3],
  ['--kind-simple', '--surface-raised', 3],
  ['--kind-down', '--surface-raised', 3],
  ['--kind-up', '--surface-raised', 3],
  ['--kind-duplex', '--surface-raised', 3],
];
