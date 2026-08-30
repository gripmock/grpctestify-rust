export interface Strip {
  scrollLeft: number;
  width: number;
  padStart: number;
  padEnd: number;
}

export interface Tab {
  left: number;
  width: number;
}

export function scrollForActive(strip: Strip, tab: Tab): number | null {
  const start = strip.scrollLeft + strip.padStart;
  const end = strip.scrollLeft + strip.width - strip.padEnd;
  if (tab.left < start) return Math.max(0, tab.left - strip.padStart);
  if (tab.left + tab.width > end) {
    return Math.max(0, tab.left + tab.width - strip.width + strip.padEnd);
  }
  return null;
}
