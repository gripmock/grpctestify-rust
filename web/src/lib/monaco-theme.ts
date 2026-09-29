export type ThemeMode = 'light' | 'dark';

export const EDITOR_THEME = 'gctf';

export function toHex(value: string): string | null {
  const text = value.trim();

  const hex = text.match(/^#([0-9a-f]{3,8})$/i);
  if (hex) {
    const digits = hex[1];
    if (digits.length === 3 || digits.length === 4) {
      return digits.slice(0, 3).split('').map(d => d + d).join('').toLowerCase();
    }
    if (digits.length === 6 || digits.length === 8) return digits.slice(0, 6).toLowerCase();
    return null;
  }

  const rgb = text.match(/^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)/);
  if (!rgb) return null;
  const channels = rgb.slice(1, 4).map(n => Math.max(0, Math.min(255, Math.round(Number(n)))));
  if (channels.some(Number.isNaN)) return null;
  return channels.map(n => n.toString(16).padStart(2, '0')).join('');
}

export interface TokenColours {
  ink: string;
  inkMuted: string;
  surface: string;
  line: string;
  accent: string;
  accentSoft: string;
  lineStrong: string;
  string: string;
  number: string;
}

function readTokens(root: HTMLElement = document.documentElement): TokenColours | null {
  const css = getComputedStyle(root);
  const pick = (name: string) => toHex(css.getPropertyValue(name));
  const ink = pick('--ink');
  const inkMuted = pick('--ink-muted');
  const surface = pick('--surface-raised');
  const line = pick('--line');
  const accent = pick('--accent');
  const lineStrong = pick('--line-strong');
  const string = pick('--kind-simple');
  const number = pick('--kind-down');
  if (!ink || !inkMuted || !surface || !line || !accent || !lineStrong || !string || !number) return null;
  return { ink, inkMuted, surface, line, accent, accentSoft: accent, lineStrong, string, number };
}

export function themeRules(t: TokenColours) {
  return [
    { token: '', foreground: t.ink },
    { token: 'string', foreground: t.string },
    { token: 'string.key.json', foreground: t.ink, fontStyle: 'bold' },
    { token: 'string.value.json', foreground: t.string },
    { token: 'number', foreground: t.number },
    { token: 'keyword', foreground: t.accent },
    { token: 'keyword.json', foreground: t.accent },
    { token: 'comment', foreground: t.inkMuted, fontStyle: 'italic' },
    { token: 'delimiter', foreground: t.inkMuted },
    { token: 'type', foreground: t.number },
    { token: 'variable', foreground: t.accent },
  ];
}

export function themeColors(t: TokenColours, _mode: ThemeMode) {
  return {
    'editor.background': `#${t.surface}`,
    'editor.foreground': `#${t.ink}`,
    'editorLineNumber.foreground': `#${t.line}`,
    'editorLineNumber.activeForeground': `#${t.inkMuted}`,
    'editorIndentGuide.background1': `#${t.line}`,
    'editorGutter.background': `#${t.surface}`,
    'editorWidget.background': `#${t.surface}`,
    'editorWidget.border': `#${t.line}`,
    'editorHoverWidget.background': `#${t.surface}`,
    'editorHoverWidget.border': `#${t.line}`,
    'editor.lineHighlightBorder': `#${t.line}`,
    'editorBracketMatch.border': `#${t.accent}`,
    'editorBracketMatch.background': `#${t.surface}`,
    'scrollbarSlider.background': `#${t.lineStrong}55`,
    'scrollbarSlider.hoverBackground': `#${t.lineStrong}99`,
  };
}

export function applyEditorTheme(monaco: any, mode: ThemeMode, root?: HTMLElement): boolean {
  const tokens = readTokens(root);
  if (!tokens) return false;
  monaco.editor.defineTheme(EDITOR_THEME, {
    base: mode === 'dark' ? 'vs-dark' : 'vs',
    inherit: true,
    rules: themeRules(tokens),
    colors: themeColors(tokens, mode),
  });
  monaco.editor.setTheme(EDITOR_THEME);
  return true;
}

let instance: any = null;

export function registerMonaco(monaco: any, mode: ThemeMode) {
  instance = monaco;
  applyEditorTheme(monaco, mode);
}

export function retheme(mode: ThemeMode) {
  if (instance) applyEditorTheme(instance, mode);
}
