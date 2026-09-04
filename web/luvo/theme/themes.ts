/** The theme system: a palette, and the mode it is shown in.
 *
 *  A palette is the product's colour identity — Terminal's amber, Slate's cool
 *  blue — and each one exists in daylight and at night. Choosing a look and
 *  choosing light or dark are two questions, and merging them into one list of
 *  four names made a reader compare "Paper" against "Terminal" as if they were
 *  alternatives rather than the same palette at two times of day.
 *
 *  A palette's colours live in `tokens.css` under `[data-theme="<id>-<mode>"]`,
 *  which is the only thing written on the document. */

export type Mode = 'light' | 'dark';
export type PaletteId = 'terminal' | 'slate' | 'plum' | 'mono';
/** What the user chose for the mode. `system` follows the OS. */
export type ModePref = Mode | 'system';

export interface Palette {
  id: PaletteId;
  label: string;
  note: string;
}

export interface ThemeChoice {
  palette: PaletteId;
  mode: ModePref;
}

export const PALETTES: readonly Palette[] = [
  { id: 'terminal', label: 'Terminal', note: 'amber' },
  { id: 'slate', label: 'Slate', note: 'cool blue' },
  { id: 'plum', label: 'Plum', note: 'violet' },
  /* The one palette with no hue of its own: pass, fail and the four call
     shapes stay in colour, and nothing else does. */
  { id: 'mono', label: 'Mono', note: 'greyscale' },
];

export const MODES: readonly { id: ModePref; label: string }[] = [
  { id: 'system', label: 'System' },
  { id: 'light', label: 'Light' },
  { id: 'dark', label: 'Dark' },
];

/** Where the choice is remembered. The app names it, because the key belongs to
 *  the app's storage rather than to the design system. */
export let THEME_KEY = 'luvo-theme';

export function rememberThemeUnder(key: string): void {
  THEME_KEY = key;
}

export function paletteOf(id: PaletteId | string): Palette {
  return PALETTES.find(p => p.id === id) ?? PALETTES[0];
}

/** The `data-theme` value for a palette in a mode. */
export function themeId(palette: PaletteId, mode: Mode): string {
  return `${palette}-${mode}`;
}

/** Every theme that exists, which is what the contrast grader walks. */
export function allThemes(): { id: string; palette: PaletteId; mode: Mode }[] {
  return PALETTES.flatMap(p =>
    (['light', 'dark'] as const).map(mode => ({ id: themeId(p.id, mode), palette: p.id, mode })));
}

/** The stored choice, including every value the app wrote before palettes and
 *  modes were told apart. */
export function readChoice(raw: string | null | undefined): ThemeChoice {
  if (raw) {
    try {
      const saved = JSON.parse(raw) as Partial<ThemeChoice>;
      if (saved && typeof saved === 'object') {
        return {
          palette: PALETTES.some(p => p.id === saved.palette) ? saved.palette! : 'terminal',
          mode: saved.mode === 'light' || saved.mode === 'dark' ? saved.mode : 'system',
        };
      }
    } catch { /* one of the older, plainer values */ }
  }
  switch (raw) {
    case 'light': case 'paper': return { palette: 'terminal', mode: 'light' };
    case 'dark': case 'terminal': return { palette: 'terminal', mode: 'dark' };
    case 'slate': case 'contrast': return { palette: 'slate', mode: 'dark' };
    default: return { palette: 'terminal', mode: 'system' };
  }
}

export function prefersDark(): boolean {
  return typeof window !== 'undefined' && !!window.matchMedia
    && window.matchMedia('(prefers-color-scheme: dark)').matches;
}

export function resolveMode(mode: ModePref, dark: boolean): Mode {
  return mode === 'system' ? (dark ? 'dark' : 'light') : mode;
}

/** One attribute, always a resolved palette-and-mode: no rule in `tokens.css`
 *  has to know about the OS, and the grader reads the blocks the browser does. */
export function applyTheme(choice: ThemeChoice): Mode {
  const mode = resolveMode(choice.mode, prefersDark());
  if (typeof document !== 'undefined') {
    document.documentElement.setAttribute('data-theme', themeId(choice.palette, mode));
  }
  return mode;
}

export function watchSystemTheme(onChange: () => void): () => void {
  if (typeof window === 'undefined' || !window.matchMedia) return () => {};
  const mq = window.matchMedia('(prefers-color-scheme: dark)');
  const handler = () => onChange();
  mq.addEventListener('change', handler);
  return () => mq.removeEventListener('change', handler);
}

/** The keyboard has one theme key: it steps light → dark → system. The palette
 *  is a choice, not a cycle — it is made once and lived in. */
export function nextMode(mode: ModePref): ModePref {
  return mode === 'light' ? 'dark' : mode === 'dark' ? 'system' : 'light';
}
