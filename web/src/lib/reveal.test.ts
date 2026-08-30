import { describe, it, expect } from 'vitest';
import { revealDelta } from './reveal';

describe('revealDelta', () => {
  it('leaves a pane that already shows enough', () => {
    expect(revealDelta(200, 500, 855)).toBe(0);
  });

  it('scrolls an outcome up from below the fold', () => {
    expect(revealDelta(829, 600, 855)).toBe(444);
  });

  it('brings a short pane fully into view and no further', () => {
    expect(revealDelta(800, 120, 855)).toBe(65);
  });

  it('scrolls back down to a pane that went off the top', () => {
    expect(revealDelta(-400, 600, 855)).toBe(-785);
  });

  it('answers for the short window this was found on', () => {
    expect(revealDelta(426, 224, 472)).toBe(178);
  });
});
