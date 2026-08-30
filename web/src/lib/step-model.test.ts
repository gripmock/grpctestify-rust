import { describe, it, expect } from 'vitest';
import { parsedForStep } from './step-model';
import type { CollectionParsed, DocumentSummary } from './types';

const head: CollectionParsed = {
  endpoint: 'a.A/One', address: 'localhost:1', headers: { h: '1' }, bodies: ['{}'],
  asserts: ['.ok'], extracts: { t: '.t' }, meta_name: 'flow', meta_tags: ['smoke'],
  meta_owner: 'qa', meta_summary: 'sum', meta_links: [], tls: {}, options: { timeout: '5' },
  bench: { mode: 'fixed' }, proto: {}, dataset: [{ id: '1' }], attributes: [],
  expect_responses: [], expect_error: null,
};

const step: DocumentSummary = {
  index: 1, endpoint: 'b.B/Two', kind: 'server', address: 'localhost:2',
  address_source: 'section', headers: { auth: 'x' }, bodies: ['{"n":2}'],
  asserts: ['.items'], extracts: { cursor: '.c' }, options: {}, tls: {}, proto: {},
  produces: ['cursor'], consumes: ['t'],
};

describe('editing one step of a chain', () => {
  it('takes what belongs to the step from the step', () => {
    const p = parsedForStep(head, step);
    expect(p.endpoint).toBe('b.B/Two');
    expect(p.headers).toEqual({ auth: 'x' });
    expect(p.bodies).toEqual(['{"n":2}']);
    expect(p.asserts).toEqual(['.items']);
    expect(p.extracts).toEqual({ cursor: '.c' });
    expect(p.options).toEqual({});
  });

  it('keeps what belongs to the file', () => {
    const p = parsedForStep(head, step);
    expect(p.meta_name).toBe('flow');
    expect(p.meta_tags).toEqual(['smoke']);
    expect(p.bench).toEqual({ mode: 'fixed' });
    expect(p.dataset).toEqual([{ id: '1' }]);
  });

  it('leaves the address empty when the step inherits it', () => {
    const p = parsedForStep(head, { ...step, address_source: 'inherited' });
    expect(p.address).toBe('');
  });

  it('gives an empty step one empty message to edit', () => {
    const p = parsedForStep(head, { ...step, bodies: [] });
    expect(p.bodies).toEqual(['']);
  });
});
