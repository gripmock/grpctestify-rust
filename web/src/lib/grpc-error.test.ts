import { describe, it, expect } from 'vitest';
import { errorText } from './grpc-error';

describe('what an error says', () => {
  it('keeps the message and drops the code the chip already shows', () => {
    expect(errorText("gRPC error code=5 message=Method 'Nope' not found")).toBe("Method 'Nope' not found");
  });

  it('leaves anything else exactly as it came', () => {
    expect(errorText('connection refused')).toBe('connection refused');
    expect(errorText('message #1 is not valid JSON: trailing characters')).toBe(
      'message #1 is not valid JSON: trailing characters',
    );
  });

  it('keeps a multi-line message whole', () => {
    expect(errorText('gRPC error code=13 message=first\nsecond')).toBe('first\nsecond');
  });
});
