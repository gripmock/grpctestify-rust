import { describe, it, expect } from 'vitest';
import { previewRequest } from './preview-request';

describe('previewRequest', () => {
  it('says which step of the chain the forms are holding', () => {
    const body = previewRequest({ endpoint: 'GET /b' }, { path: 'two.httf', originalPath: 'two.httf', activeStep: 1 });
    expect(body.document_index).toBe(1);
    expect(body.original_path).toBe('two.httf');
  });

  it('carries the head of a single-document file', () => {
    expect(previewRequest({}, { path: 'one.gctf', activeStep: 0 }).document_index).toBe(0);
  });

  it('leaves a draft without an original to stitch into', () => {
    expect(previewRequest({}, { path: 'draft.gctf', originalPath: null, activeStep: 0 }).original_path).toBeUndefined();
  });
});
