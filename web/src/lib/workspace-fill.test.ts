import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const CSS = readFileSync(join(import.meta.dirname, '..', 'app.css'), 'utf8');

function ruleFor(selector: string): string {
  const at = CSS.indexOf(selector + ' {');
  expect(at, `${selector} has no rule`).toBeGreaterThan(-1);
  return CSS.slice(at, CSS.indexOf('}', at));
}

describe('the stacked workspace', () => {
  it('lets the outcome pane take what is left under a form that has not run', () => {
    const rule = ruleFor('.workspace.is-rows.is-idle.is-form > .response-pane');
    expect(rule).toContain('flex: 1 1 auto');
    expect(rule).toContain('min-height: 5rem');
  });

  it('gives an idle editor the room instead', () => {
    const rule = ruleFor('.workspace.is-rows.is-idle:not(.is-form) > .response-pane');
    expect(rule).toContain('flex: 0 0 auto');
    expect(rule).toContain('min-height: 5rem');
  });

  it('keeps the request pane sized by its content there', () => {
    expect(ruleFor('.workspace.is-rows.is-idle:not(.is-form) > .request-pane')).toContain('height: auto');
  });

  it('never shrinks the request pane past its own controls', () => {
    const rule = ruleFor('.workspace.is-rows.is-idle:not(.is-form) > .request-pane');
    expect(rule).not.toContain('min-height: 0');
    expect(rule).toContain('min-height: 9rem');
  });

  it('lets it take what is left once something has', () => {
    expect(ruleFor('.workspace.is-rows:not(.is-idle) > .response-pane')).toContain('flex: 1 1 auto');
  });
});

describe('the three steps to a first call', () => {
  it('are one grid, so the three sentences start in the same place', () => {
    const rule = ruleFor('.start-steps ol');
    expect(rule).toContain('display: grid');
    expect(ruleFor('.start-step')).toContain('display: contents');
  });

  it('read from the left', () => {
    const rule = ruleFor('.start-step .start-detail');
    expect(rule).toContain('text-align: left');
    expect(rule).toContain('justify-self: start');
  });
});
