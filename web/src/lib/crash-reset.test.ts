import { describe, expect, it } from 'vitest';
import { isViewState } from './crash-reset';

describe('what a reset clears', () => {
  it('clears the tab strip and the rest of the view', () => {
    for (const key of ['grpctestify-tabs', 'play.layout', 'play.drawer.h', 'play.chain.open', 'play.requestPct']) {
      expect(isViewState(key)).toBe(true);
    }
  });

  it('keeps the work: history, environments, settings, totals', () => {
    for (const key of [
      'grpctestify-history',
      'grpctestify-envs',
      'grpctestify-active-env',
      'grpctestify-settings',
      'grpctestify-totals',
      'grpctestify-recent-addresses',
      'grpctestify-session',
    ]) {
      expect(isViewState(key)).toBe(false);
    }
  });

  it('keeps what is not ours', () => {
    expect(isViewState('vite-ui-theme')).toBe(false);
    expect(isViewState('playwright')).toBe(false);
  });
});
