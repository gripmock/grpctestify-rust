export function moveItem<T>(items: T[], from: number, to: number): T[] {
  if (from === to) return items;
  if (from < 0 || from >= items.length) return items;
  if (to < 0 || to >= items.length) return items;
  const next = [...items];
  const [moved] = next.splice(from, 1);
  next.splice(to, 0, moved);
  return next;
}

export function duplicateItem<T>(items: T[], index: number): T[] {
  if (index < 0 || index >= items.length) return items;
  return [...items.slice(0, index + 1), items[index], ...items.slice(index + 1)];
}
