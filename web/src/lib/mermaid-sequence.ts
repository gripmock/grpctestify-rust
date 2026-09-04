export interface SequenceStep {
  from: string;
  to: string;
  label: string;
  dashed: boolean;
}

export interface Sequence {
  participants: string[];
  steps: SequenceStep[];
}

const ARROW = /^\s*([A-Za-z0-9_]+)\s*(-{1,2}>>?)\s*([A-Za-z0-9_]+)\s*:\s*(.*)$/;

export function parseSequence(text: string): Sequence | null {
  const lines = text.split('\n').map(l => l.trim()).filter(Boolean);
  if (lines[0] !== 'sequenceDiagram') return null;

  const participants: string[] = [];
  const steps: SequenceStep[] = [];
  for (const line of lines.slice(1)) {
    const participant = line.match(/^participant\s+(.+)$/);
    if (participant) {
      participants.push(participant[1].trim());
      continue;
    }
    const arrow = line.match(ARROW);
    if (!arrow) return null;
    const [, from, kind, to, label] = arrow;
    if (!participants.includes(from)) participants.push(from);
    if (!participants.includes(to)) participants.push(to);
    steps.push({ from, to, label: label.trim(), dashed: kind.startsWith('--') });
  }
  return steps.length > 0 ? { participants, steps } : null;
}
