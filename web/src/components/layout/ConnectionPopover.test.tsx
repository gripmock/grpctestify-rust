import { describe, expect, it, beforeEach } from 'vitest';
import { ConnectionPopover } from './ConnectionPopover';
import { ToastProvider } from 'luvo/ui/ToastContext';
import { useStore } from '../../lib/store';
import { mount } from 'luvo/test/render';

const chip = (
  <ToastProvider>
    <ConnectionPopover />
  </ToastProvider>
);

describe('a run that would dial elsewhere', () => {
  beforeEach(() => {
    useStore.setState({
      workspacePath: 'noaddr.gctf',
      request: { endpoint: 'a.A/One', headers: {}, bodies: ['{}'] },
      collectionParsed: null,
      documents: [],
      activeStep: 0,
      protocol: 'grpc',
      tls: false,
      tlsInsecure: false,
      address: '',
      addressTouched: false,
      environments: [],
      activeEnvironment: null,
      serverEnv: { address: null },
      projectEnvs: [],
      projectDefaults: null,
    } as never);
  });

  it('marks the chip when the header aims somewhere the run will not', () => {
    useStore.setState({ address: '127.0.0.1:4790', addressTouched: true } as never);
    const ui = mount(chip);
    const button = ui.get('.conn-chip');
    expect(button.className).toContain('is-warn');
    expect(button.getAttribute('title')).toContain('A run of this file dials');
    ui.unmount();
  });

  it('leaves the chip alone when both go to the same place', () => {
    useStore.setState({
      address: '',
      addressTouched: false,
      serverEnv: { address: 'localhost:4770' },
    } as never);
    const ui = mount(chip);
    const button = ui.get('.conn-chip');
    expect(button.className).not.toContain('is-warn');
    ui.unmount();
  });
});

describe('what the chip says a call goes out on', () => {
  const parsedWith = (over: Record<string, unknown>) => ({
    endpoint: 'a.A/One', address: '', headers: {}, bodies: ['{}'],
    asserts: [], extracts: {}, meta_name: null, meta_tags: [], meta_owner: null,
    meta_summary: null, meta_links: [], tls: {}, options: {}, bench: {}, proto: {},
    dataset: [], attributes: [], expect_responses: [], expect_error: null,
    ...over,
  });

  beforeEach(() => {
    useStore.setState({
      workspacePath: 'secure.gctf',
      request: { endpoint: 'a.A/One', headers: {}, bodies: ['{}'] },
      documents: [], activeStep: 0,
      protocol: 'grpc', tls: false, tlsInsecure: false,
      address: '127.0.0.1:4790', addressTouched: true,
      environments: [], activeEnvironment: null,
      serverEnv: { address: null }, projectEnvs: [], projectDefaults: null,
      collectionParsed: null,
    } as never);
  });

  it('says what the file says, not what the header holds', () => {
    useStore.setState({ collectionParsed: parsedWith({ tls: { insecure: 'true' } }) } as never);
    const ui = mount(chip);
    const text = ui.get('.conn-chip').textContent ?? '';
    expect(text).toContain('insecure');
    expect(text).not.toContain('plaintext');
    ui.unmount();
  });

  it('says the transport the file names', () => {
    useStore.setState({ collectionParsed: parsedWith({ options: { protocol: 'grpc-web' } }) } as never);
    expect(mount(chip).get('.conn-chip').textContent).toContain('grpc-web');
  });

  it('says the environment when the environment carries one', () => {
    useStore.setState({
      collectionParsed: parsedWith({}),
      environments: [{ name: 'staging', source: 'browser', variables: {}, tls: true, tlsInsecure: false }],
      activeEnvironment: 'staging',
    } as never);
    const text = mount(chip).get('.conn-chip').textContent ?? '';
    expect(text).toContain('tls');
    expect(text).not.toContain('plaintext');
  });

  it('lets the file win over the environment', () => {
    useStore.setState({
      collectionParsed: parsedWith({ tls: {} }),
      environments: [{ name: 'staging', source: 'browser', variables: {}, tls: true, tlsInsecure: false }],
      activeEnvironment: 'staging',
    } as never);
    useStore.setState({ collectionParsed: parsedWith({ tls: { insecure: 'true' } }) } as never);
    expect(mount(chip).get('.conn-chip').textContent).toContain('insecure');
  });

  it('says the header when the file says nothing', () => {
    useStore.setState({ collectionParsed: parsedWith({}), tls: true, tlsInsecure: false } as never);
    const text = mount(chip).get('.conn-chip').textContent ?? '';
    expect(text).toContain('tls');
    expect(text).not.toContain('insecure');
  });
});
