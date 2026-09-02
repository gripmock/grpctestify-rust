export const DRAWER_MIN_H = 260;
export const DRAWER_MAX_H = 620;

export function drawerHeight(kept: number, viewport: number): number {
  const room = Math.max(DRAWER_MIN_H, Math.round(viewport * 0.5));
  return Math.min(Math.max(DRAWER_MIN_H, kept), DRAWER_MAX_H, room);
}
