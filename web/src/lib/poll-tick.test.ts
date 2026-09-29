import { describe, expect, it } from 'vitest';
import { mtimeMoved, pollsWhile, SYNC_EVERY, syncsAnyway } from './poll-tick';

describe('when the workbench asks the server about its files', () => {
  it('only while the tab is on screen', () => {
    expect(pollsWhile('visible')).toBe(true);
    expect(pollsWhile('hidden')).toBe(false);
  });
});

describe('what a tick does with the mtime it was given', () => {
  it('does nothing while nothing on disk has moved', () => {
    expect(mtimeMoved(1700, 1700)).toBe(false);
  });

  it('does nothing when the server did not say', () => {
    expect(mtimeMoved(1700, undefined)).toBe(false);
    expect(mtimeMoved(undefined, undefined)).toBe(false);
  });

  it('resyncs once the mtime moved, or is first heard of', () => {
    expect(mtimeMoved(1700, 1701)).toBe(true);
    expect(mtimeMoved(undefined, 1700)).toBe(true);
  });
});

describe('reading the open files even when nothing said they moved', () => {
  it('asks anyway every so often, because the counter can stop moving', () => {
    expect(syncsAnyway(1)).toBe(false);
    expect(syncsAnyway(SYNC_EVERY - 1)).toBe(false);
    expect(syncsAnyway(SYNC_EVERY)).toBe(true);
    expect(syncsAnyway(SYNC_EVERY * 2)).toBe(true);
  });

  it('does not count the tick before the first one', () => {
    expect(syncsAnyway(0)).toBe(false);
  });

  it('keeps the fallback rare next to the poll it rides on', () => {
    expect(SYNC_EVERY).toBeGreaterThanOrEqual(5);
  });
});
