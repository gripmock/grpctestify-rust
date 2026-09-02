import { ancestorsOf } from './tree';

export interface DirToggles {
  open: Set<string>;
  closed: Set<string>;
  at: string | null;
}

export const NO_TOGGLES: DirToggles = { open: new Set(), closed: new Set(), at: null };

function settled(toggles: DirToggles, selected: string | null): DirToggles {
  if (toggles.at === selected || selected === null) return toggles;
  const holders = new Set(ancestorsOf(selected));
  return {
    open: toggles.open,
    closed: new Set([...toggles.closed].filter(dir => !holders.has(dir))),
    at: selected,
  };
}

export function expandedDirs(roots: string[], selected: string | null, toggles: DirToggles): Set<string> {
  const now = settled(toggles, selected);
  const wanted = [...roots, ...(selected ? ancestorsOf(selected) : []), ...now.open];
  return new Set(wanted.filter(dir => !now.closed.has(dir)));
}

export function toggleDir(toggles: DirToggles, selected: string | null, dir: string, expanded: boolean): DirToggles {
  const now = settled(toggles, selected);
  const open = new Set(now.open);
  const closed = new Set(now.closed);
  if (expanded) { open.delete(dir); closed.add(dir); }
  else { closed.delete(dir); open.add(dir); }
  return { open, closed, at: selected };
}
