export type TreeStep = 'next' | 'prev' | 'first' | 'last';

export function treeStep(key: string): TreeStep | null {
  switch (key) {
    case 'ArrowDown': return 'next';
    case 'ArrowUp': return 'prev';
    case 'Home': return 'first';
    case 'End': return 'last';
    default: return null;
  }
}

export function stepIndex(current: number, count: number, step: TreeStep): number {
  if (count === 0) return -1;
  switch (step) {
    case 'first': return 0;
    case 'last': return count - 1;
    case 'next': return Math.min(current + 1, count - 1);
    case 'prev': return Math.max(current - 1, 0);
  }
}

export function rowIsTabStop(path: string, selected: string | null, first: string | null): boolean {
  return selected === null ? path === first : path === selected;
}

export function moveRowFocus(from: HTMLElement, step: TreeStep, selector: string): void {
  const rows = [...document.querySelectorAll<HTMLElement>(selector)];
  const next = stepIndex(rows.indexOf(from), rows.length, step);
  rows[next]?.focus();
}
