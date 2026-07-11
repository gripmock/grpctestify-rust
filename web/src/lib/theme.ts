

export const colors = {
  accent: '#3b82f6',
  accentHover: '#2563eb',
  accentActive: '#1d4ed8',
  success: '#22c55e',
  successBg: '#22c55e18',
  error: '#ef4444',
  errorBg: '#ef444418',
  warning: '#f59e0b',
  warningBg: '#f59e0b18',
} as const;

export const css = {
  
  flexCenter: { display: 'flex', alignItems: 'center', justifyContent: 'center' } as const,
  flexBetween: { display: 'flex', alignItems: 'center', justifyContent: 'space-between' } as const,
  truncate: { overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' } as const,
  mono: { fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace' } as const,

  
  label: {
    fontSize: 10, fontWeight: 600, color: 'var(--text-muted)',
    textTransform: 'uppercase', letterSpacing: '0.6px',
  } as const,

  
  badge: (bg: string, fg: string): React.CSSProperties => ({
    display: 'inline-flex', alignItems: 'center', gap: 3,
    fontSize: 10, fontWeight: 500, padding: '1px 6px', borderRadius: 4,
    background: bg, color: fg,
  }),

  
  sectionHeader: {
    ...{ fontSize: 10, fontWeight: 600, color: 'var(--text-muted)',
    textTransform: 'uppercase', letterSpacing: '0.6px' },
  } as React.CSSProperties,
} as const;


export function btn(variant: 'primary' | 'danger' | 'ghost' | 'default' = 'default', size: 'sm' | 'md' = 'md') {
  const pad = size === 'sm' ? { padding: '4px 10px' } : { padding: '8px 16px' };
  const fSize = size === 'sm' ? 12 : 13;

  const base: React.CSSProperties = {
    display: 'inline-flex', alignItems: 'center', justifyContent: 'center', gap: 5,
    fontWeight: 500, fontSize: fSize, borderRadius: 6, cursor: 'pointer',
    transition: 'all 0.15s ease', userSelect: 'none', whiteSpace: 'nowrap',
    border: 'none', outline: 'none', textDecoration: 'none', lineHeight: 1.3,
    ...pad,
  };

  switch (variant) {
    case 'primary':
      return {
        ...base, background: colors.accent, color: '#fff',
        boxShadow: '0 1px 3px rgba(59,130,246,0.25)',
      } as React.CSSProperties;
    case 'danger':
      return { ...base, background: colors.error, color: '#fff' } as React.CSSProperties;
    case 'ghost':
      return { ...base, background: 'transparent', color: 'var(--text-secondary)', border: 'none' } as React.CSSProperties;
    default:
      return {
        ...base, background: 'var(--bg-tertiary)', color: 'var(--text-primary)',
        border: '1px solid var(--border)',
      } as React.CSSProperties;
  }
}


export const input: React.CSSProperties = {
  padding: '8px 12px', fontSize: 13, borderRadius: 6,
  border: '1px solid var(--border)', background: 'var(--bg-primary)',
  color: 'var(--text-primary)', outline: 'none', transition: 'border-color 0.15s ease',
};


export function hoverBg(el: HTMLElement, color: string) { el.style.background = color; }
export function resetBg(el: HTMLElement) { el.style.background = ''; }
