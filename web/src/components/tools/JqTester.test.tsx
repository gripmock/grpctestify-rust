import { describe, expect, it, beforeEach } from 'vitest';
import { JqTester } from './JqTester';
import { ToastProvider } from 'luvo/ui/ToastContext';
import { useStore } from '../../lib/store';
import { mount } from 'luvo/test/render';
import { forgetScratch } from '../../lib/tool-scratch';

const drawer = (
  <ToastProvider>
    <JqTester seed={null} />
  </ToastProvider>
);

describe('whose REQUEST the filter reads', () => {
  beforeEach(() => {
    useStore.setState({ documents: [], activeStep: 0, request: { endpoint: 'a.A/One', headers: {}, bodies: [] } } as never);
  });

  it('names the step in a chain', () => {
    useStore.setState({ documents: [{ index: 0 }, { index: 1 }], activeStep: 0 } as never);
    const ui = mount(drawer);
    expect(ui.byText("this step 1's request")).toHaveLength(1);
    ui.unmount();
  });

  it('follows the step being edited', () => {
    useStore.setState({ documents: [{ index: 0 }, { index: 1 }], activeStep: 1 } as never);
    const ui = mount(drawer);
    expect(ui.byText("this step 2's request")).toHaveLength(1);
    ui.unmount();
  });

  it('says the file when the file is one document', () => {
    useStore.setState({ documents: [{ index: 0 }], activeStep: 0 } as never);
    const ui = mount(drawer);
    expect(ui.byText("this file's request")).toHaveLength(1);
    ui.unmount();
  });

  it('offers the step without saying "this step 2"', () => {
    useStore.setState({
      documents: [{ index: 0 }, { index: 1 }], activeStep: 1,
      request: { endpoint: 'a.A/Two', headers: {}, bodies: ['{"a":1}'] },
    } as never);
    const ui = mount(drawer);
    const option = ui.byText("this step 2's request")[0].closest('button');
    expect(option?.getAttribute('title')).toBe('The message step 2 would send');
    ui.unmount();
  });

  it('refuses in the same words it offers', () => {
    useStore.setState({ documents: [{ index: 0 }, { index: 1 }], activeStep: 1 } as never);
    const ui = mount(drawer);
    const option = ui.byText("this step 2's request")[0].closest('button');
    expect(option?.getAttribute('title')).toBe('Step 2 has no REQUEST yet');
    ui.unmount();
  });
});

describe('a filter handed over by a failed check', () => {
  beforeEach(() => {
    forgetScratch();
    useStore.setState({ documents: [], activeStep: 0, request: { endpoint: 'a.A/One', headers: {}, bodies: [] } } as never);
  });

  const held = (ui: ReturnType<typeof mount>) =>
    (ui.container.querySelector('textarea, input.field.mono') as HTMLInputElement | null)?.value;

  it('arrives in the box', () => {
    const ui = mount(
      <ToastProvider>
        <JqTester seed={{ message: 'Hello' }} handed={{ expr: '.message', n: 1 }} />
      </ToastProvider>,
    );
    expect(held(ui)).toBe('.message');
    ui.unmount();
  });

  it('arrives again when it is asked for again', () => {
    const first = mount(
      <ToastProvider>
        <JqTester seed={{ message: 'Hello' }} handed={{ expr: '.message', n: 1 }} />
      </ToastProvider>,
    );
    first.unmount();
    const again = mount(
      <ToastProvider>
        <JqTester seed={{ message: 'Hello' }} handed={{ expr: '.message | length', n: 2 }} />
      </ToastProvider>,
    );
    expect(held(again)).toBe('.message | length');
    again.unmount();
  });

  it('leaves the box alone when nothing was handed over', () => {
    const ui = mount(
      <ToastProvider>
        <JqTester seed={{ message: 'Hello' }} />
      </ToastProvider>,
    );
    expect(held(ui)).toBe('.');
    ui.unmount();
  });
});
