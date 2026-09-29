import { describe, it, expect } from 'vitest';
import { groupSectionsByStep, sameAsFirst, sectionLines } from './section-spans';

const span = (section: string, start: number) => ({ section, start_line: start, end_line: start + 3, content: '' });

describe('grouping what the parser saw', () => {
  it('splits at every endpoint after the first', () => {
    const groups = groupSectionsByStep([
      span('META', 1), span('ADDRESS', 6), span('ENDPOINT', 9), span('REQUEST', 12),
      span('ENDPOINT', 22), span('REQUEST', 25),
      span('ENDPOINT', 40), span('ASSERTS', 43),
    ]);
    expect(groups.map(g => g.step)).toEqual([1, 2, 3]);
    expect(groups[0].sections.map(s => s.section)).toEqual(['META', 'ADDRESS', 'ENDPOINT', 'REQUEST']);
    expect(groups[1].sections.map(s => s.section)).toEqual(['ENDPOINT', 'REQUEST']);
    expect(groups[2].sections.map(s => s.section)).toEqual(['ENDPOINT', 'ASSERTS']);
  });

  it('keeps a single document in one group', () => {
    const groups = groupSectionsByStep([span('ENDPOINT', 1), span('REQUEST', 4)]);
    expect(groups).toHaveLength(1);
  });

  it('has nothing to group when the parser saw nothing', () => {
    expect(groupSectionsByStep([])).toEqual([]);
  });
});

describe('runtimes that repeat', () => {
  const opt = (value: string) => [{ key: 'timeout', value, source: 'CLI default' }];

  it('marks a step that resolves to exactly what step one did', () => {
    expect(sameAsFirst([opt('30 s'), opt('30 s'), opt('11 s')])).toEqual([false, true, false]);
  });

  it('never marks the first step', () => {
    expect(sameAsFirst([opt('30 s')])).toEqual([false]);
  });
});

describe('the lines a section occupies', () => {
  it('reads as the gutter reads', () => {
    expect(sectionLines({ start_line: 1, end_line: 3 })).toBe('1–3');
  });

  it('says one line once', () => {
    expect(sectionLines({ start_line: 12, end_line: 12 })).toBe('12');
  });

  it('never runs backwards', () => {
    expect(sectionLines({ start_line: 5, end_line: 0 })).toBe('5');
  });
});
