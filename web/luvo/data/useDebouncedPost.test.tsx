import { describe, expect, it, vi, afterEach } from 'vitest';
import { useDebouncedPost } from './useDebouncedPost';
import { mount } from '../test/render';

function Probe({ body }: { body: unknown | null }) {
  const { data, busy } = useDebouncedPost<{ said: string }>('/api/thing', body, 1);
  return <div className="out">{busy ? 'busy' : data ? data.said : 'nothing'}</div>;
}

function Held({ body }: { body: unknown | null }) {
  const { data, busy, stale } = useDebouncedPost<{ said: string }>('/api/thing', body, 1);
  return (
    <div className="out">
      {data ? data.said : 'nothing'}
      {busy ? ' · busy' : ''}
      {stale ? ' · stale' : ''}
    </div>
  );
}

afterEach(() => { vi.unstubAllGlobals(); });

describe('a debounced post', () => {
  it('answers what the server said', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ said: 'one' }))));
    const ui = mount(<Probe body={{ a: 1 }} />);
    await new Promise(r => setTimeout(r, 20));
    expect(ui.get('.out').textContent).toBe('one');
    ui.unmount();
  });

  /* The panels share one hook across tabs: a tab with nothing to send used to
     show the previous tab's answer. */
  it('forgets the answer when there is nothing to ask', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ said: 'one' }))));
    const ui = mount(<Probe body={{ a: 1 }} />);
    await new Promise(r => setTimeout(r, 20));
    expect(ui.get('.out').textContent).toBe('one');

    ui.update(<Probe body={null} />);
    await new Promise(r => setTimeout(r, 20));
    expect(ui.get('.out').textContent).toBe('nothing');
    ui.unmount();
  });
});

describe('two answers racing', () => {
  /* The sequence was checked before the body was parsed, so a slow answer that
     lost the race still landed on top of the newer one it lost to. */
  it('keeps the newer answer when an older one finishes last', async () => {
    let release: (() => void) | null = null;
    const slow = new Promise<void>(r => { release = r; });
    let call = 0;
    vi.stubGlobal('fetch', vi.fn(async () => {
      call += 1;
      if (call === 1) {
        return {
          ok: true,
          status: 200,
          json: async () => { await slow; return { said: 'first' }; },
          text: async () => '',
        } as unknown as Response;
      }
      return new Response(JSON.stringify({ said: 'second' }));
    }));

    const ui = mount(<Probe body={{ a: 1 }} />);
    await new Promise(r => setTimeout(r, 5));
    ui.update(<Probe body={{ a: 2 }} />);
    await new Promise(r => setTimeout(r, 20));
    expect(ui.get('.out').textContent).toBe('second');

    release!();
    await new Promise(r => setTimeout(r, 20));
    expect(ui.get('.out').textContent).toBe('second');
    ui.unmount();
  });
});

describe('a question that changes while the last answer stands', () => {
  it('keeps the answer on screen until the next one lands, and says it is stale', async () => {
    let release: (() => void) | null = null;
    const slow = new Promise<void>(r => { release = r; });
    let call = 0;
    vi.stubGlobal('fetch', vi.fn(async () => {
      call += 1;
      if (call === 1) return new Response(JSON.stringify({ said: 'first' }));
      return {
        ok: true,
        status: 200,
        json: async () => { await slow; return { said: 'second' }; },
        text: async () => '',
      } as unknown as Response;
    }));

    const ui = mount(<Held body={{ a: 1 }} />);
    await new Promise(r => setTimeout(r, 20));
    expect(ui.get('.out').textContent).toBe('first');

    ui.update(<Held body={{ a: 2 }} />);
    await new Promise(r => setTimeout(r, 5));
    expect(ui.get('.out').textContent).toBe('first · busy · stale');

    release!();
    await new Promise(r => setTimeout(r, 20));
    expect(ui.get('.out').textContent).toBe('second');
    ui.unmount();
  });

  it('holds nothing back once there is nothing to ask', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ said: 'one' }))));
    const ui = mount(<Held body={{ a: 1 }} />);
    await new Promise(r => setTimeout(r, 20));
    expect(ui.get('.out').textContent).toBe('one');

    ui.update(<Held body={null} />);
    expect(ui.get('.out').textContent).toBe('nothing');
    ui.unmount();
  });

  it('is busy but not stale before any answer has landed', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ said: 'one' }))));
    const ui = mount(<Held body={{ a: 1 }} />);
    expect(ui.get('.out').textContent).toBe('nothing · busy');
    await new Promise(r => setTimeout(r, 20));
    ui.unmount();
  });

  it('says the pending question is busy after nothing was asked', async () => {
    let release: (() => void) | null = null;
    const slow = new Promise<void>(r => { release = r; });
    let call = 0;
    vi.stubGlobal('fetch', vi.fn(async () => {
      call += 1;
      if (call === 1) return new Response(JSON.stringify({ said: 'first' }));
      return {
        ok: true,
        status: 200,
        json: async () => { await slow; return { said: 'second' }; },
        text: async () => '',
      } as unknown as Response;
    }));

    const ui = mount(<Probe body={{ a: 1 }} />);
    await new Promise(r => setTimeout(r, 20));
    expect(ui.get('.out').textContent).toBe('first');

    ui.update(<Probe body={null} />);
    expect(ui.get('.out').textContent).toBe('nothing');

    ui.update(<Probe body={{ a: 2 }} />);
    await new Promise(r => setTimeout(r, 5));
    expect(ui.get('.out').textContent).toBe('busy');

    release!();
    await new Promise(r => setTimeout(r, 20));
    expect(ui.get('.out').textContent).toBe('second');
    ui.unmount();
  });
});
