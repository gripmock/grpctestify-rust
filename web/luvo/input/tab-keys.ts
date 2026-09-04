/** Where an arrow key moves inside a tab strip.
 *
 *  `null` when the key means nothing here, so the handler can leave the event
 *  alone rather than swallowing keys it does not use. Wraps at both ends: a
 *  strip is a ring, and stopping at the edge makes the last tab harder to reach
 *  than the first. */
export function nextTabIndex(current: number, count: number, key: string): number | null {
  if (count === 0) return null;
  switch (key) {
    case 'ArrowRight':
    case 'ArrowDown':
      return (current + 1) % count;
    case 'ArrowLeft':
    case 'ArrowUp':
      return (current - 1 + count) % count;
    case 'Home':
      return 0;
    case 'End':
      return count - 1;
    default:
      return null;
  }
}

/** Where a tab dropped on another one lands: before it, or after it when the
 *  pointer is past that tab's middle. */
export function dropIndex(over: number, after: boolean): number {
  return after ? over + 1 : over;
}
