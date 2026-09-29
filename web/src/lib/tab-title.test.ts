import { describe, expect, it } from 'vitest';
import { tabTitle, titleIsBorrowed } from './tab-title';

const tab = (over: Partial<{ label: string; endpoint: string }> = {}) =>
  ({ label: 'Untitled', endpoint: '', ...over });

describe('what a tab is called', () => {
  it('is what it holds while it has no name of its own', () => {
    expect(tabTitle(tab({ endpoint: 'GET /api/health' }))).toBe('GET /api/health');
    expect(tabTitle(tab({ endpoint: 'helloworld.Greeter/SayHello' }))).toBe('SayHello');
  });

  it('is the file name once there is one', () => {
    expect(tabTitle({ label: 'login.gctf', endpoint: 'a.A/One' })).toBe('login.gctf');
  });

  it('is still Untitled while the tab holds nothing', () => {
    expect(tabTitle(tab())).toBe('Untitled');
    expect(tabTitle(tab({ endpoint: '   ' }))).toBe('Untitled');
  });

  it('follows what is being typed in the tab on screen', () => {
    expect(tabTitle(tab({ endpoint: 'a.A/Old' }), 'a.A/New')).toBe('New');
  });

  it('says when the name is borrowed rather than given', () => {
    expect(titleIsBorrowed(tab({ endpoint: 'GET /a' }))).toBe(true);
    expect(titleIsBorrowed(tab())).toBe(false);
    expect(titleIsBorrowed({ label: 'login.gctf', endpoint: 'a.A/One' })).toBe(false);
  });
});
