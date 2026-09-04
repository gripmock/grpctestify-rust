import { describe, it, expect } from 'vitest';
import { pathPlaceholderNote } from './path-placeholder';

describe('a placeholder typed into a path field', () => {
  it('is named, once, however many fields carry it', () => {
    const note = pathPlaceholderNote('{{CA}}/ca.pem', 'certs/{{CA}}.pem', './client.pem');
    expect(note).toContain('{{CA}}');
    expect(note!.match(/\{\{CA\}\}/g)).toHaveLength(1);
    expect(note).toContain('read from this file');
  });

  it('says nothing about ordinary paths', () => {
    expect(pathPlaceholderNote('certs/ca.pem', '', undefined, null)).toBe(null);
  });

  it('ignores braces that are not a variable', () => {
    expect(pathPlaceholderNote('weird{{ }}name.pem')).toBe(null);
  });
});
