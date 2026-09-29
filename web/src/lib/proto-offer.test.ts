import { describe, expect, it } from 'vitest';
import { offerNote } from './proto-offer';

describe('what an empty list of schemas says', () => {
  it('offers the way in when nothing is named', () => {
    const said = offerNote('descriptor set', false);
    expect(said).toContain('No descriptor set in this project yet');
    expect(said).toContain('drop it anywhere');
  });

  it('does not deny what the field beside it holds', () => {
    const said = offerNote('descriptor set', true);
    expect(said).not.toContain('No descriptor set');
    expect(said).toContain('this file names one of its own');
  });

  it('says the same for .proto files', () => {
    expect(offerNote('.proto', false)).toContain('No .proto in this project yet');
    expect(offerNote('.proto', true)).toContain('this file names its own');
  });
});
