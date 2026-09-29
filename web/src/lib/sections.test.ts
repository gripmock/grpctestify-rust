import { describe, it, expect } from 'vitest';
import { GCTF_SECTIONS, configKeys, hiddenSections, sectionsByGroup, visibleSections, HTTF_SECTIONS } from './sections';
import type { CollectionParsed } from './types';

function parsed(over: Partial<CollectionParsed> = {}): CollectionParsed {
  return {
    endpoint: 'pkg.Svc/M', address: '', headers: {}, bodies: ['{}'],
    asserts: [], extracts: {}, meta_name: null, meta_tags: [], meta_owner: null, meta_summary: null, meta_links: [],
    tls: {}, options: {}, bench: {}, proto: {}, dataset: [], attributes: [],
    expect_responses: [], expect_error: null,
    ...over,
  };
}

describe('visibleSections', () => {
  it('shows the document sections a .gctf always has', () => {
    const keys = visibleSections(parsed(), ['{}'], {}).map(s => s.key);
    expect(keys).toEqual(['body', 'headers', 'asserts', 'extracts', 'source', 'plan']);
  });

  it('hides proto and bench until the file has them', () => {
    const keys = visibleSections(parsed({ proto: { files: 'a.proto' } }), ['{}'], {}).map(s => s.key);
    expect(keys).toContain('proto');
    expect(keys).not.toContain('bench');
  });

  it('counts only what is worth counting', () => {
    const s = visibleSections(
      parsed({ asserts: ['.a', '.b'], extracts: { t: '.t' }, options: { timeout: '5' } }),
      ['{}', '{}'],
      { authorization: 'Bearer x' },
    );
    const count = (k: string) => s.find(x => x.key === k)?.count;
    expect(count('asserts')).toBe(2);
    expect(count('extracts')).toBe(1);
    expect(count('options')).toBe(1);
    expect(count('headers')).toBe(1);
    expect(count('body')).toBe(2);
    expect(count('meta')).toBeUndefined();
    expect(visibleSections(parsed({ meta_name: 'flow', meta_tags: ['smoke'] }), ['{}'], {}).find(x => x.key === 'meta')?.count).toBe(2);
  });

  it('does not count a single request body', () => {
    expect(visibleSections(parsed(), ['{}'], {}).find(s => s.key === 'body')?.count).toBeUndefined();
  });

  it('has no env or playground tab — those are not sections of the document', () => {
    const keys = visibleSections(parsed(), ['{}'], {}).map(s => String(s.key));
    expect(keys).not.toContain('env');
    expect(keys).not.toContain('try');
  });
});

describe('hiddenSections', () => {
  it('offers exactly the sections the file does not have yet', () => {
    expect(hiddenSections(parsed(), ['{}'], {}).map(s => s.key)).toEqual(['options', 'tls', 'meta', 'proto', 'dataset', 'bench']);
  });

  it('stops offering one once the file has it', () => {
    const keys = hiddenSections(parsed({ bench: { concurrency: '10' } }), ['{}'], {}).map(s => s.key);
    expect(keys).not.toContain('bench');
    expect(keys).toContain('dataset');
  });

  it('never offers an always-on section', () => {
    const keys = hiddenSections(parsed(), ['{}'], {}).map(s => String(s.key));
    for (const always of ['body', 'headers', 'asserts', 'source']) expect(keys).not.toContain(always);
  });
});

describe('groups', () => {
  it('puts every section in exactly one group', () => {
    const keys = GCTF_SECTIONS.map(s => s.key);
    expect(new Set(keys).size).toBe(keys.length);
    for (const def of GCTF_SECTIONS) {
      expect(['editor', 'config', 'view']).toContain(def.group);
    }
  });

  it('never leaves a section out of the strip', () => {
    const parsed = null;
    const groups = sectionsByGroup(parsed, [], {});
    const shown = [...groups.editor, ...groups.config, ...groups.view].map(s => s.key);
    const alwaysOn = GCTF_SECTIONS.filter(s => s.always).map(s => s.key);
    for (const key of alwaysOn) expect(shown).toContain(key);
  });

  it('keeps the editors, the config sections and the views apart', () => {
    const groups = sectionsByGroup(null, [], {});
    expect(groups.editor.map(s => s.key)).toEqual(['body', 'headers', 'asserts', 'extracts']);
    expect(groups.view.map(s => s.key)).toEqual(['source', 'plan']);
    expect(groups.config).toEqual([]);
    const configured = sectionsByGroup(
      parsed({ options: { timeout: '5' }, bench: { mode: 'fixed' } }),
      ['{}'],
      {},
    );
    expect(configured.config.map(s => s.key)).toEqual(['options', 'bench']);
  });
});

describe('the expect tab', () => {
  it('counts every way the file states its expectation', () => {
    const def = GCTF_SECTIONS.find(d => d.key === 'asserts')!;
    const p = parsed({
      asserts: ['.ok == true'],
      expect_responses: [
        { body: '{}', partial: false, unordered_arrays: false, with_asserts: false, tolerance: null, redact: [] },
      ],
    });
    expect(def.label).toBe('expect');
    expect(def.count?.(p, ['{}'], {})).toBe(2);
  });
});

describe('what a tab counts', () => {
  const headers = (h: Record<string, string>) =>
    GCTF_SECTIONS.find(s => s.key === 'headers')!.count!(null, ['{}'], h);

  it('counts the headers a save would write', () => {
    expect(headers({ authorization: 'Bearer x' })).toBe(1);
  });

  it('does not count a row that has no name yet', () => {
    expect(headers({ '': '' })).toBe(0);
    expect(headers({ '': '', 'x-real': '1' })).toBe(1);
  });
});

describe('the sections a family has', () => {
  it('does not offer an HTTP file a proto, a tls block or a bench', () => {
    const keys = HTTF_SECTIONS.map(s => s.key);
    expect(keys).not.toContain('proto');
    expect(keys).not.toContain('tls');
    expect(keys).not.toContain('bench');
    expect(keys).toContain('options');
    expect(keys).toContain('dataset');
    expect(keys).toContain('meta');
  });

  it('keeps every gctf section for a gctf file', () => {
    const keys = visibleSections(parsed({ proto: { files: 'a.proto' } }), ['{}'], {}, 'gctf').map(s => s.key);
    expect(keys).toContain('proto');
  });

  it('does not offer to add one that the family has no use for', () => {
    const offered = hiddenSections(parsed(), ['{}'], {}, 'httf').map(s => s.key);
    expect(offered).not.toContain('proto');
    expect(offered).not.toContain('bench');
  });
});

describe('the config tab of an HTTP file', () => {
  it('offers only what an HTTP file can carry', () => {
    expect(configKeys('httf')).toEqual(['options', 'meta', 'dataset']);
    expect(configKeys('gctf')).toContain('tls');
    expect(configKeys('gctf')).toContain('proto');
    expect(configKeys('gctf')).toContain('bench');
  });
});

describe('what a config section is for', () => {
  it('is said once per section that can be added', () => {
    const config = GCTF_SECTIONS.filter(d => d.group === 'config');
    expect(config.filter(d => !d.note?.trim()).map(d => d.key)).toEqual([]);
  });

  it('and reaches the menu that offers them', () => {
    const offered = hiddenSections(null, [], {});
    expect(offered.find(s => s.key === 'dataset')?.note).toContain('one case per row');
    expect(offered.every(s => (s.note ?? '').trim() !== '')).toBe(true);
  });

  it('is not said about a section that is always a tab', () => {
    expect(GCTF_SECTIONS.filter(d => d.always && d.note).map(d => d.key)).toEqual([]);
  });
});

describe('the meta a file carries from outside its META', () => {
  const metaCount = (over = {}) =>
    visibleSections(parsed(over), ['{}'], {}).find(s => s.key === 'meta')?.count;
  const attr = (name: string, value: string) => ({ section: 'REQUEST', index: 0, name, value });

  it('opens the row for a file whose owner and tags are attributes', () => {
    expect(metaCount({ attributes: [attr('owner', 'payments'), attr('tag', 'smoke,slow')] })).toBe(3);
  });

  it('does not count them twice when META names its own', () => {
    expect(metaCount({
      meta_owner: 'platform', meta_tags: ['api'],
      attributes: [attr('owner', 'payments'), attr('tag', 'smoke,slow')],
    })).toBe(2);
  });

  it('counts links, which are META and were left out', () => {
    expect(metaCount({ meta_links: ['https://example.test/1'] })).toBe(1);
  });
});
