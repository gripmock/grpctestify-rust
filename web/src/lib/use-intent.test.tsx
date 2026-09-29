import { describe, expect, it } from 'vitest';
import { intentShows, useIntentFlag, useIntentText } from './use-intent';
import { mount } from 'luvo/test/render';

function Flag({ intent }: { intent: number }) {
  const [shown, set] = useIntentFlag(intent);
  return (
    <div>
      <span className="state">{shown ? 'open' : 'closed'}</span>
      <button className="open" onClick={() => set(true)}>open</button>
      <button className="close" onClick={() => set(false)}>close</button>
    </div>
  );
}

function Text({ intent, prefill }: { intent: number; prefill: string }) {
  const [text, set] = useIntentText(intent, prefill);
  return <input value={text} onChange={e => set(e.target.value)} />;
}

describe('a flag raised by a counter in the store', () => {
  it('stays down for the count it was born with', () => {
    expect(intentShows({ open: false, seen: 3 }, 3)).toBe(false);
  });

  it('rises when the count moves, and is lowered by closing', () => {
    const ui = mount(<Flag intent={1} />);
    expect(ui.get('.state').textContent).toBe('closed');
    ui.update(<Flag intent={2} />);
    expect(ui.get('.state').textContent).toBe('open');
    ui.click('.close');
    expect(ui.get('.state').textContent).toBe('closed');
    ui.update(<Flag intent={3} />);
    expect(ui.get('.state').textContent).toBe('open');
    ui.unmount();
  });

  it('can be raised and lowered by hand between counts', () => {
    const ui = mount(<Flag intent={1} />);
    ui.click('.open');
    expect(ui.get('.state').textContent).toBe('open');
    ui.click('.close');
    expect(ui.get('.state').textContent).toBe('closed');
    ui.unmount();
  });
});

describe('text prefilled by a counter in the store', () => {
  it('shows the prefill until typed over, and again when the count moves', () => {
    const ui = mount(<Text intent={1} prefill="grpcurl a" />);
    expect((ui.get('input') as HTMLInputElement).value).toBe('grpcurl a');
    ui.type('input', 'curl b');
    expect((ui.get('input') as HTMLInputElement).value).toBe('curl b');
    ui.update(<Text intent={1} prefill="grpcurl a" />);
    expect((ui.get('input') as HTMLInputElement).value).toBe('curl b');
    ui.update(<Text intent={2} prefill="grpcurl c" />);
    expect((ui.get('input') as HTMLInputElement).value).toBe('grpcurl c');
    ui.unmount();
  });
});
