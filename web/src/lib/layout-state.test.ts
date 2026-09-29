import { describe, it, expect } from 'vitest';
import { COLUMNS_FIT, columnsFit, tabFills, workspaceClass } from './layout-state';

describe('which tabs stretch', () => {
  it('lets the editors fill the pane', () => {
    expect(tabFills('body')).toBe(true);
    expect(tabFills('source')).toBe(true);
  });

  it('leaves every form at its own height', () => {
    for (const tab of ['config', 'headers', 'asserts', 'extracts', 'plan', 'meta'] as const) {
      expect(tabFills(tab), tab).toBe(false);
    }
  });
});

describe('what the workspace says about itself', () => {
  it('marks a form so the page scrolls instead of the pane', () => {
    expect(workspaceClass('rows', 'config', true)).toBe('workspace is-rows is-form');
  });

  it('marks the idle state so an empty outcome pane keeps only a strip', () => {
    expect(workspaceClass('rows', 'body', false)).toBe('workspace is-rows is-idle');
  });

  it('can be both at once', () => {
    expect(workspaceClass('rows', 'plan', false)).toBe('workspace is-rows is-idle is-form');
  });

  it('says nothing extra when an editor has an outcome beside it', () => {
    expect(workspaceClass('columns', 'body', true)).toBe('workspace is-columns');
  });

  it('marks the pane as a box once it has a height of its own', () => {
    expect(workspaceClass('rows', 'body', true)).toBe('workspace is-rows is-boxed');
  });

  it('leaves a form alone — it is read down the page, not in a box', () => {
    expect(workspaceClass('rows', 'config', true)).toBe('workspace is-rows is-form');
  });
});

describe('a pane the user sized', () => {
  it('is marked once the handle has been dragged', () => {
    expect(workspaceClass('rows', 'body', false, true)).toBe('workspace is-rows is-idle is-sized is-boxed');
  });

  it('is unmarked until then, so the content decides', () => {
    expect(workspaceClass('rows', 'body', false)).toBe('workspace is-rows is-idle');
  });
});

describe('the width the side-by-side layout needs', () => {
  it('is the one the stylesheet asks for', () => {
    expect(COLUMNS_FIT).toBe('(min-width: 64rem)');
    expect(columnsFit(1024)).toBe(true);
    expect(columnsFit(1023)).toBe(false);
    expect(columnsFit(1024, 20)).toBe(false);
  });
});
