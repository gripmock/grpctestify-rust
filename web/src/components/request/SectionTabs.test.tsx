import { afterEach, describe, expect, it } from 'vitest';
import { SectionBody, SectionTabs } from './SectionTabs';
import { ToastProvider } from 'luvo/ui/ToastContext';
import { ModalProvider } from 'luvo/ui/ModalContext';
import { useStore } from '../../lib/store';
import { mount } from 'luvo/test/render';

const sections = () => (
  <ToastProvider>
    <ModalProvider>
      <SectionTabs />
      <SectionBody fill={false} />
    </ModalProvider>
  </ToastProvider>
);

describe('the section on screen', () => {
  afterEach(() => { useStore.setState({ requestTab: 'body' } as never); });

  it('is the panel of the tab that is on', () => {
    useStore.setState({ requestTab: 'headers', collectionParsed: null, request: { endpoint: 'a.B/C', headers: {}, bodies: ['{}'] } } as never);
    const ui = mount(sections());
    const on = ui.get('[role="tab"][aria-selected="true"]');
    const panel = ui.get('[role="tabpanel"]');
    expect(panel.getAttribute('aria-labelledby')).toBe(on.id);
    expect(on.getAttribute('aria-controls')).toBe(panel.id);
    expect(panel.className).toContain('section-body');
    ui.unmount();
  });
});
