import type { DocumentSummary } from './types';

export interface PlanFacts {
  steps: number;
  target: string | null;
  targets: number;
  asserts: number;
  expected: number;
  variables: number;
  expectsError: boolean;
  streaming: boolean;
}

export function planFacts(
  documents: DocumentSummary[],
  expectsError: boolean[],
  expectedResponses: number[] = [],
  running: { asserts: number; variables: number }[] = [],
): PlanFacts {
  const addresses = [...new Set(documents.map(d => d.address).filter(Boolean))];
  const sum = (pick: (r: { asserts: number; variables: number }) => number, fallback: number) =>
    (running.length === documents.length ? running.reduce((n, r) => n + pick(r), 0) : fallback);
  return {
    steps: documents.length,
    target: addresses.length === 1 ? addresses[0] : null,
    targets: addresses.length,
    asserts: sum(r => r.asserts, documents.reduce((n, d) => n + d.asserts.length, 0)),
    expected: expectedResponses.reduce((n, count) => n + count, 0),
    variables: sum(r => r.variables, documents.reduce((n, d) => n + Object.keys(d.extracts).length, 0)),
    expectsError: expectsError.some(Boolean),
    streaming: documents.some(d => d.kind !== 'unary'),
  };
}

export function planHeadline(facts: PlanFacts): string {
  const parts: string[] = [];
  parts.push(facts.steps === 1 ? '1 step' : `${facts.steps} steps`);
  if (facts.target) parts.push(facts.target);
  else if (facts.targets > 1) parts.push(`${facts.targets} targets`);
  if (facts.asserts > 0) parts.push(facts.asserts === 1 ? '1 assert' : `${facts.asserts} asserts`);
  if (facts.expected > 0) {
    parts.push(facts.expected === 1 ? '1 expected response' : `${facts.expected} expected responses`);
  }
  if (facts.asserts === 0 && facts.expected === 0 && !facts.expectsError) parts.push('nothing checked');
  if (facts.variables > 0) {
    parts.push(facts.variables === 1 ? '1 variable' : `${facts.variables} variables`);
  }
  if (facts.streaming) parts.push('streaming');
  if (facts.expectsError) parts.push('expects an error');
  return parts.join(' · ');
}

export function stepAsserts(blocks: { assertions?: string[]; skipped?: boolean }[]): number {
  return blocks
    .filter(block => !block.skipped)
    .reduce((n, block) => n + (block.assertions?.length ?? 0), 0);
}

export function stepSkips(plan: {
  requests?: { skipped?: boolean }[];
  expectations?: { skipped?: boolean; expectation_type?: string }[];
  assertions?: { skipped?: boolean }[];
  extractions?: { skipped?: boolean }[];
}): string[] {
  const out: string[] = [];
  for (const [i, r] of (plan.requests ?? []).entries()) {
    if (r.skipped) out.push((plan.requests ?? []).length > 1 ? `REQUEST ${i + 1}` : 'REQUEST');
  }
  for (const e of plan.expectations ?? []) {
    if (e.skipped) out.push(e.expectation_type === 'error' ? 'ERROR' : 'RESPONSE');
  }
  if ((plan.assertions ?? []).some(a => a.skipped)) out.push('ASSERTS');
  if ((plan.extractions ?? []).some(e => e.skipped)) out.push('EXTRACT');
  return out;
}
