import { describe, expect, it, beforeEach } from 'vitest';
import { BenchResults } from './BenchResults';
import { ToastProvider } from 'luvo/ui/ToastContext';
import { useStore } from '../../lib/store';
import { emptyRun } from '../../lib/jobs';
import { mount } from 'luvo/test/render';

const panel = (
  <ToastProvider>
    <BenchResults />
  </ToastProvider>
);

function benchRun(summary: Record<string, number>) {
  return {
    ...emptyRun(),
    kind: 'bench' as const,
    durationMs: 172,
    benchReport: { summary, latency_distribution: [] },
  };
}

describe('the two axes a bench reports', () => {
  beforeEach(() => {
    useStore.setState({ benchOverUnsaved: [] } as never);
  });

  it('says the transport side only when it differs', () => {
    useStore.setState({ run: benchRun({ rps_observed: 2352, passed: 300, failed: 0, ok: 300, errors: 0 }) } as never);
    const ui = mount(panel);
    const text = ui.get('.tiles').textContent ?? '';
    expect(text).not.toContain('ok / errors');
    ui.unmount();
  });

  it('shows both when a request failed but the file passed', () => {
    useStore.setState({ run: benchRun({ rps_observed: 10, passed: 300, failed: 0, ok: 0, errors: 300 }) } as never);
    const ui = mount(panel);
    expect(ui.get('.tiles').textContent).toContain('ok / errors');
    ui.unmount();
  });
});
