import { describe, expect, it, beforeEach, vi } from 'vitest';
import { ChainRail } from './ChainRail';
import { ToastProvider } from 'luvo/ui/ToastContext';
import { useStore } from '../../lib/store';
import { emptyRun } from '../../lib/jobs';
import type { DocumentSummary } from '../../lib/types';
import { mount } from 'luvo/test/render';

function step(over: Partial<DocumentSummary> = {}): DocumentSummary {
  return {
    index: 0, endpoint: 'a.A/One', kind: 'unary', address: 'localhost:4770',
    address_source: 'section', headers: {}, bodies: ['{}'], asserts: [], extracts: {},
    options: {}, tls: {}, proto: {}, produces: [], consumes: [],
    ...over,
  };
}

const chain = (
  <ToastProvider>
    <ChainRail />
  </ToastProvider>
);

describe('the chain rail', () => {
  beforeEach(() => {
    useStore.setState({
      documents: [],
      activeStep: 0,
      workspacePath: 'a.gctf',
      runJobId: null,
      run: emptyRun(),
    });
  });

  it('draws no rail for a file with one step', () => {
    useStore.setState({ documents: [step()] });
    const ui = mount(chain);
    expect(ui.all('.rail-dot')).toHaveLength(0);
    expect(ui.all('.step-row')).toHaveLength(0);
    ui.unmount();
  });

  it('offers a file with one step the way to a second', () => {
    useStore.setState({ documents: [step()] });
    const ui = mount(chain);
    const add = ui.all('button').find(b => b.textContent?.trim() === 'step');
    expect(add?.getAttribute('title')).toContain('this file becomes a chain');
    expect(add?.hasAttribute('disabled')).toBe(false);
    ui.unmount();
  });

  it('says a chain lives in a file when there is none to add to', () => {
    useStore.setState({ documents: [step()], workspacePath: null });
    const ui = mount(chain);
    const add = ui.all('button').find(b => b.textContent?.trim() === 'step');
    expect(add?.getAttribute('title')).toContain('Save the file first');
    expect(add?.hasAttribute('disabled')).toBe(true);
    ui.unmount();
  });

  it('says nothing at all before a file has been read', () => {
    useStore.setState({ documents: [] });
    const ui = mount(chain);
    expect(ui.container.innerHTML).toBe('');
    ui.unmount();
  });

  it('draws a dot per step, and marks the one being edited', () => {
    useStore.setState({ documents: [step({ index: 0 }), step({ index: 1 }), step({ index: 2 })], activeStep: 1 });
    const ui = mount(chain);
    const dots = ui.all('.rail-dot');
    expect(dots).toHaveLength(3);
    expect(dots.map(d => d.getAttribute('aria-pressed'))).toEqual(['false', 'true', 'false']);
    ui.unmount();
  });

  it('names the step a run stopped at', () => {
    useStore.setState({
      documents: [step({ index: 0 }), step({ index: 1 }), step({ index: 2 })],
      run: {
        ...emptyRun(),
        verdicts: { 'a.gctf': { path: 'a.gctf', state: 'fail', documents: [4, 7] } },
      },
    });
    const ui = mount(chain);
    expect(ui.get('.chain-bar').textContent).toContain('stopped at step 2');
    expect(ui.all('.rail-dot').map(d => d.className)).toEqual([
      'rail-dot is-pass is-on', 'rail-dot is-fail', 'rail-dot is-skip',
    ]);
    ui.unmount();
  });

  it('moves to the step whose dot was pressed', () => {
    useStore.setState({ documents: [step({ index: 0 }), step({ index: 1 })] });
    const ui = mount(chain);
    ui.click(ui.all('.rail-dot')[1]);
    expect(useStore.getState().activeStep).toBe(1);
    ui.unmount();
  });

  it('refuses to move away from an edited step, and says why', () => {
    useStore.setState({
      documents: [step({ index: 0 }), step({ index: 1 })],
      selectStep: vi.fn().mockReturnValue(false),
    });
    const ui = mount(chain);
    ui.click(ui.all('.rail-dot')[1]);
    expect(document.body.textContent).toContain('Save or discard this step');
    ui.unmount();
  });

  it('will not run a file that has never been saved', () => {
    useStore.setState({ documents: [step({ index: 0 }), step({ index: 1 })], workspacePath: null });
    const ui = mount(chain);
    const run = ui.all('button[title*="Save the file first"]')[0] as HTMLButtonElement;
    expect(run.disabled).toBe(true);
    expect(run.title).toContain('Save the file first');
    ui.unmount();
  });

  it('offers the remove button on the step being edited, and there only', () => {
    useStore.setState({
      documents: [step(), step({ index: 1, endpoint: 'a.A/Two' })],
      activeStep: 1,
    });
    const ui = mount(chain);
    ui.click('.chain [aria-expanded]');
    const rows = ui.all('.step-row');
    expect(rows[1].querySelector('.step-remove')).not.toBe(null);
    expect(rows[0].querySelector('.step-remove')).toBe(null);
    ui.unmount();
  });

  it('offers no remove button where there is nothing to remove', () => {
    useStore.setState({ documents: [step()], activeStep: 0 });
    const ui = mount(chain);
    ui.click('.chain [aria-expanded]');
    expect(ui.all('.step-remove')).toHaveLength(0);
    ui.unmount();
  });
});
