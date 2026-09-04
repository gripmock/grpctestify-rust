import { describe, it, expect } from 'vitest';
import { everyMessageSkipped, messageRun, sectionRun } from './message-attributes';
import type { CollectionParsed, SectionAttribute } from './types';

function parsed(attributes: SectionAttribute[], bodies_stream = false): CollectionParsed {
  return { attributes, bodies_stream } as unknown as CollectionParsed;
}

const at = (index: number, name: string, value = 'true'): SectionAttribute =>
  ({ section: 'REQUEST', index, name, value });

describe('what the file says about one message', () => {
  it('marks the message a run skips, and only that one', () => {
    const p = parsed([at(1, 'skip')]);
    expect(messageRun(p, 0).skipped).toBe(false);
    expect(messageRun(p, 1).skipped).toBe(true);
  });

  it('reads a repeat of more than one', () => {
    expect(messageRun(parsed([at(0, 'repeat', '3')]), 0).repeat).toBe(3);
    expect(messageRun(parsed([at(0, 'repeat', '1')]), 0).repeat).toBeNull();
  });

  it('ignores an attribute on another section', () => {
    expect(messageRun(parsed([{ section: 'ASSERTS', index: 0, name: 'skip', value: 'true' }]), 0).skipped)
      .toBe(false);
  });

  it('shares one section’s attributes across a stream written in it', () => {
    const p = parsed([at(0, 'skip')], true);
    expect(messageRun(p, 0).skipped).toBe(true);
    expect(messageRun(p, 2).skipped).toBe(true);
  });

  it('says nothing about a file that carries no attributes', () => {
    expect(messageRun(null, 0)).toEqual({ skipped: false, repeat: null });
  });
});

describe('a section a run walks past', () => {
  it('reads the skip on the section it is asked about', () => {
    const p = parsed([{ section: 'ASSERTS', index: 0, name: 'skip', value: 'true' }]);
    expect(sectionRun(p, 'ASSERTS').skipped).toBe(true);
    expect(sectionRun(p, 'EXTRACT').skipped).toBe(false);
  });
});

describe('an attribute the HTTP runner does not act on', () => {
  it('is not promised for an HTTP file', () => {
    const p = parsed([at(0, 'repeat', '3'), at(0, 'skip')]);
    expect(messageRun(p, 0, 'httf')).toEqual({ skipped: true, repeat: null });
    expect(messageRun(p, 0, 'gctf')).toEqual({ skipped: true, repeat: 3 });
  });
});

describe('a skipped expectation', () => {
  const parsed = (section: string) => ({
    attributes: [{ section, index: 0, name: 'skip', value: 'true' }],
  }) as never;

  it('is seen on RESPONSE and on ERROR', () => {
    expect(sectionRun(parsed('RESPONSE'), 'RESPONSE').skipped).toBe(true);
    expect(sectionRun(parsed('ERROR'), 'ERROR').skipped).toBe(true);
  });

  it('is not read across the two', () => {
    expect(sectionRun(parsed('RESPONSE'), 'ERROR').skipped).toBe(false);
    expect(sectionRun(parsed('ERROR'), 'RESPONSE').skipped).toBe(false);
  });
});

describe('a file that skips every message', () => {
  it('is only that when none of them is left', () => {
    expect(everyMessageSkipped(parsed([at(0, 'skip')]), 1)).toBe(true);
    expect(everyMessageSkipped(parsed([at(0, 'skip')]), 2)).toBe(false);
    expect(everyMessageSkipped(parsed([at(0, 'skip'), at(1, 'skip')]), 2)).toBe(true);
  });

  it('reads one streaming section for all of its messages', () => {
    expect(everyMessageSkipped(parsed([at(0, 'skip')], true), 3)).toBe(true);
    expect(everyMessageSkipped(parsed([], true), 3)).toBe(false);
  });

  it('says nothing for a file with no message and nothing for HTTP', () => {
    expect(everyMessageSkipped(parsed([at(0, 'skip')]), 0)).toBe(false);
    expect(everyMessageSkipped(parsed([at(0, 'skip')]), 1, 'httf')).toBe(false);
  });
});
