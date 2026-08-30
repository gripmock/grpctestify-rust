import { describe, expect, it } from 'vitest';
import { delayUnused, overruledBy } from './options-override';
import type { SectionAttribute } from './types';

const attr = (section: string, name: string, value = 'true'): SectionAttribute =>
  ({ section, index: 0, name, value });

describe('what a section attribute takes from the OPTIONS line', () => {
  it('names the section that carries it', () => {
    expect(overruledBy([attr('RESPONSE', 'timeout', '5')], 'timeout'))
      .toEqual({ section: 'RESPONSE', value: '5' });
  });

  it('reads either spelling of a two-word attribute', () => {
    expect(overruledBy([attr('REQUEST', 'retry-delay', '0.5')], 'retry_delay')?.value).toBe('0.5');
    expect(overruledBy([attr('REQUEST', 'no_retry')], 'no_retry')?.value).toBe('true');
  });

  it('takes the first of several', () => {
    expect(overruledBy([attr('REQUEST', 'timeout', '2'), attr('RESPONSE', 'timeout', '9')], 'timeout'))
      .toEqual({ section: 'REQUEST', value: '2' });
  });

  it('says nothing about an option no attribute speaks for', () => {
    expect(overruledBy([attr('RESPONSE', 'skip')], 'timeout')).toBeNull();
    expect(overruledBy([], 'retry')).toBeNull();
  });
});

describe('a delay between attempts that are never made', () => {
  it('is unused when nothing retries', () => {
    expect(delayUnused({ retry_delay: '2' }, [])).toBe(true);
    expect(delayUnused({ retry_delay: '2', retry: '0' }, [])).toBe(true);
  });

  it('is unused when the file turns retries off', () => {
    expect(delayUnused({ retry_delay: '2', retry: '3', no_retry: 'true' }, [])).toBe(true);
    expect(delayUnused({ retry_delay: '2', retry: '3' }, [attr('REQUEST', 'no_retry')])).toBe(true);
  });

  it('is read when there is a second attempt to wait for', () => {
    expect(delayUnused({ retry_delay: '2', retry: '3' }, [])).toBe(false);
    expect(delayUnused({ retry_delay: '2' }, [attr('REQUEST', 'retry', '2')])).toBe(false);
  });

  it('says nothing when no delay is written', () => {
    expect(delayUnused({ retry: '0' }, [])).toBe(false);
  });
});
