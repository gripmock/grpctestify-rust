/** Where a menu opened at a point actually goes.
 *
 *  A menu anchored at the pointer runs off the window when the pointer is near
 *  an edge — the last item of a menu opened at the bottom of a long rail was
 *  unreachable. It is pulled back inside, and flipped above the point when
 *  there is no room below. */
export interface Box {
  width: number;
  height: number;
}

export interface Viewport {
  width: number;
  height: number;
}

export function placeMenu(at: { x: number; y: number }, box: Box, view: Viewport, margin = 8): { left: number; top: number } {
  const left = Math.max(margin, Math.min(at.x, view.width - box.width - margin));
  const fitsBelow = at.y + box.height + margin <= view.height;
  const top = fitsBelow
    ? at.y
    : Math.max(margin, at.y - box.height);
  return { left, top };
}
