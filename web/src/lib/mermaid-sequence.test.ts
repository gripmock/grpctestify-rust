import { describe, it, expect } from 'vitest';
import { parseSequence } from './mermaid-sequence';

const DIAGRAM = `sequenceDiagram
    participant Client
    participant Server
    Client->>Server: SayHello
    Server-->>Client: response`;

describe('the sequence diagrams docs writes', () => {
  it('reads participants and both arrow forms', () => {
    expect(parseSequence(DIAGRAM)).toEqual({
      participants: ['Client', 'Server'],
      steps: [
        { from: 'Client', to: 'Server', label: 'SayHello', dashed: false },
        { from: 'Server', to: 'Client', label: 'response', dashed: true },
      ],
    });
  });

  it('refuses what it does not understand', () => {
    expect(parseSequence('graph TD\n  A --> B')).toBeNull();
    expect(parseSequence('sequenceDiagram\n  loop every minute\n  end')).toBeNull();
    expect(parseSequence('sequenceDiagram\n participant A')).toBeNull();
  });
});
