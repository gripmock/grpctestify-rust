import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const CSS = readFileSync(join(import.meta.dirname, '..', 'app.css'), 'utf8');

function ruleFor(selector: string): string {
  const at = CSS.indexOf(selector + ' {');
  expect(at, `${selector} has no rule`).toBeGreaterThan(-1);
  return CSS.slice(at, CSS.indexOf('}', at));
}

describe('the rail', () => {
  it('does not scroll as a whole', () => {
    const rule = ruleFor('.sidebar-body');
    expect(rule).toContain('overflow: hidden');
    expect(rule).toContain('flex-direction: column');
  });

  it('gives each panel the height to divide', () => {
    expect(ruleFor('.rail-panel, .history')).toContain('min-height: 0');
  });

  it('scrolls the lists instead', () => {
    const rule = ruleFor('.rail-panel > .tree, .history > .history-calls');
    expect(rule).toContain('overflow: auto');
    expect(rule).toContain('flex: 1');
  });
});

describe('what a run leaves in the rail', () => {
  it('wraps its summary rather than shortening the report names', () => {
    const rule = ruleFor('.summary.run-summary');
    expect(rule).toContain('flex-wrap: wrap');
    expect(ruleFor('.summary.run-summary .run-report')).toContain('flex: 0 0 auto');
  });

  it('gives a failure two lines to say what happened', () => {
    const rule = ruleFor('.run-failure-line');
    expect(rule).toContain('line-clamp: 2');
    expect(rule).not.toContain('white-space: nowrap');
  });
});
