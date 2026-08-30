import { describe, expect, it, beforeEach } from 'vitest';
import { RunControl, RunSummary } from './RunBar';
import { ToastProvider } from 'luvo/ui/ToastContext';
import { ModalProvider } from 'luvo/ui/ModalContext';
import { useStore } from '../../lib/store';
import { mount } from 'luvo/test/render';
import { emptyRun } from '../../lib/jobs';

const bar = (
  <ModalProvider>
    <ToastProvider>
      <RunControl />
    </ToastProvider>
  </ModalProvider>
);

describe('the control that starts a run', () => {
  beforeEach(() => {
    useStore.setState({
      visibleFiles: ['a.gctf', 'b.gctf'],
      workspacePath: 'a.gctf',
      runScope: 'all',
      runJobId: null,
      run: emptyRun(),
      tabs: [],
      runData: null,
    });
  });

  it('says nothing about a source while there is none', () => {
    const ui = mount(bar);
    expect(ui.all('.run-over')).toHaveLength(0);
    ui.unmount();
  });

  it('names the source every file is run over', () => {
    useStore.setState({ runData: 'data/paths.csv' });
    const ui = mount(bar);
    const chip = ui.get('.run-over');
    expect(chip.textContent).toContain('paths.csv');
    expect(chip.getAttribute('title')).toBe('Every file runs once per row of data/paths.csv');
    ui.unmount();
  });
});

describe('a source that is gone', () => {
  beforeEach(() => {
    useStore.setState({
      visibleFiles: ['a.gctf'],
      workspacePath: 'a.gctf',
      runScope: 'file',
      runJobId: null,
      run: emptyRun(),
      tabs: [],
      runData: 'paths.csv',
      runError: 'Data source not found: paths.csv',
    });
  });

  it('offers the way out of a run that cannot start', () => {
    const ui = mount(
      <ModalProvider><ToastProvider><RunSummary /></ToastProvider></ModalProvider>,
    );
    const out = ui.all('button').find(b => b.textContent?.includes('run without it'));
    expect(out).toBeDefined();
    ui.click(out!);
    expect(useStore.getState().runData).toBeNull();
    ui.unmount();
  });
});
