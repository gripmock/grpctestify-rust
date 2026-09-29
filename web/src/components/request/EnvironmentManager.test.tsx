import { afterEach, describe, expect, it, vi } from 'vitest';
import { act } from 'react';
import { EnvironmentManager } from './EnvironmentManager';
import { ToastProvider } from 'luvo/ui/ToastContext';
import { ModalProvider } from 'luvo/ui/ModalContext';
import { useStore } from '../../lib/store';
import { mount } from 'luvo/test/render';

const manager = (props: { defineVar?: string; defineValue?: string } = {}) => (
  <ToastProvider>
    <ModalProvider>
      <EnvironmentManager onClose={() => {}} {...props} />
    </ModalProvider>
  </ToastProvider>
);

describe('what the manager opens on', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    useStore.setState({ browserEnvs: [], projectEnvs: [], environments: [], activeEnvironment: null, projectRoot: null } as never);
  });

  it('starts a new environment around the name it was asked to define, on the first paint', () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('[]', { status: 200 })));
    useStore.setState({ browserEnvs: [], projectEnvs: [], environments: [], activeEnvironment: null } as never);
    const ui = mount(manager({ defineVar: 'API_KEY', defineValue: 'k-1' }));
    expect(ui.container.textContent).toContain('Create');
    expect(ui.container.textContent).toContain('Kept in this browser');
    ui.unmount();
  });

  it('verifies certificates for a TLS environment that never said otherwise', () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('[]', { status: 200 })));
    const dev = { name: 'dev', source: 'browser' as const, variables: {}, tls: true };
    useStore.setState({ browserEnvs: [dev], projectEnvs: [], environments: [dev], activeEnvironment: 'dev' } as never);
    const ui = mount(manager({ defineVar: 'TOKEN' }));
    const skip = ui.all('input[type="checkbox"]') as HTMLInputElement[];
    expect(skip).toHaveLength(1);
    expect(skip[0].checked).toBe(false);
    ui.unmount();
  });
});

describe('a variable the project says is a credential', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    useStore.setState({ browserEnvs: [], projectEnvs: [], environments: [], activeEnvironment: null, projectRoot: null } as never);
  });

  it('is hidden in the editor even when nothing about the word says so', async () => {
    vi.stubGlobal('fetch', vi.fn(async (url: unknown) => {
      const at = String(url);
      if (at.includes('/local')) return new Response(JSON.stringify({ exists: false, content: null, secret: [] }), { status: 200 });
      if (at.includes('/api/project/env/')) return new Response(JSON.stringify({ content: 'SEED=abc\nHOST=api.test\n', secret: ['SEED'] }), { status: 200 });
      return new Response('[]', { status: 200 });
    }));
    const dev = { name: 'dev', source: 'project' as const, variables: { SEED: 'abc', HOST: 'api.test' } };
    useStore.setState({ projectRoot: '.grpctestify', browserEnvs: [], projectEnvs: [dev], environments: [dev], activeEnvironment: 'dev' } as never);
    const ui = mount(manager({ defineVar: 'SEED' }));
    await act(async () => { await Promise.resolve(); await Promise.resolve(); await Promise.resolve(); });
    const values = ui.all('.var-line input:nth-child(2)') as HTMLInputElement[];
    const seed = values.find(v => v.value === 'abc');
    const host = values.find(v => v.value === 'api.test');
    expect(seed?.value).toBe('abc');
    expect(seed?.type).toBe('password');
    expect(host?.type).toBe('text');
    ui.unmount();
  });
});
