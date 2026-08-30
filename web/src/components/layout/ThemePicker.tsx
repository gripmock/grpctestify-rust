import { useCallback, useState } from 'react';
import { Seg } from 'luvo/ui/Seg';
import { Check, ChevronDown, Monitor, Moon, Sun } from 'lucide-react';
import { useStore } from '../../lib/store';
import { useDismiss } from 'luvo/input/useDismiss';
import { Popover } from 'luvo/ui/Popover';
import { MODES, PALETTES, type ModePref, type PaletteId } from 'luvo/theme/themes';

const MODE_ICON = { system: Monitor, light: Sun, dark: Moon } as const;

export function ThemePicker() {
  const palette = useStore(s => s.palette);
  const mode = useStore(s => s.mode);
  const themeMode = useStore(s => s.themeMode);
  const setPalette = useStore(s => s.setPalette);
  const setMode = useStore(s => s.setMode);
  const [open, setOpen] = useState(false);
  const close = useCallback(() => setOpen(false), []);
  const ref = useDismiss<HTMLDivElement>(open, close);

  const chosen = PALETTES.find(p => p.id === palette);
  const goingTo = themeMode === 'dark' ? 'light' : 'dark';
  const Icon = MODE_ICON[goingTo];

  return (
    <div className="picker theme-picker" ref={ref}>
      <button
        className="btn is-ghost is-icon is-sm"
        onClick={() => setMode(goingTo)}
        aria-label={`Switch ${chosen?.label} to ${goingTo}`}
        title={`${chosen?.label} — ${mode === 'system' ? `following the system (${themeMode})` : themeMode}. Click for ${goingTo}.`}
      >
        <Icon size={13} />
      </button>
      <button
        className="btn is-ghost is-icon is-sm theme-more"
        onClick={() => setOpen(o => !o)}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label="Palette"
        title="Palette"
      >
        <ChevronDown size={11} />
      </button>

      <Popover open={open} anchor={ref} align="end" className="theme-menu">
        <div className="menu" role="menu">
          <div className="menu-group">palette</div>
          {PALETTES.map((p: { id: PaletteId; label: string; note: string }) => (
            <button
              key={p.id}
              className={`menu-item${palette === p.id ? ' is-on' : ''}`}
              role="menuitemradio"
              aria-checked={palette === p.id}
              onClick={() => { setPalette(p.id); setOpen(false); }}
            >
              <span className="theme-swatch" data-theme={`${p.id}-${themeMode}`} aria-hidden="true" />
              <span className="grow">{p.label}</span>
              <span className="muted">{p.note}</span>
              {palette === p.id && <Check size={11} />}
            </button>
          ))}

          <div className="menu-group">light or dark</div>
          <Seg
            className="theme-modes"
            label="Light or dark"
            value={mode}
            onChange={setMode}
            options={MODES.map((m: { id: ModePref; label: string }) => {
              const ModeIcon = MODE_ICON[m.id];
              return {
                value: m.id,
                label: <><ModeIcon size={11} /> {m.label}</>,
                title: m.id === 'system' ? `Follows the system — ${themeMode} right now` : undefined,
              };
            })}
          />
        </div>
      </Popover>
    </div>
  );
}
