import type { DocumentSummary } from './types';

export interface DiagramStep {
  index: number;
  request: string;
  response: string;
  binds: string[];
  streaming: boolean;
  parallel: boolean;
}

export interface DiagramGroup {
  start: number;
  end: number;
}

export function groupsOf(steps: { parallel: boolean }[]): DiagramGroup[] {
  const groups: DiagramGroup[] = [];
  let i = 0;
  while (i < steps.length) {
    if (!steps[i].parallel) { i++; continue; }
    let end = i;
    while (end < steps.length && steps[end].parallel) end++;
    if (end - i > 1) groups.push({ start: i, end: end - 1 });
    i = end;
  }
  return groups;
}

export interface DiagramModel {
  server: string;
  steps: DiagramStep[];
}

export interface StepSummary {
  error_expected: boolean;
  total_responses: number;
  running?: { checks: number; binds: string[] };
}

export function chainDiagram(documents: DocumentSummary[], summaries: StepSummary[]): DiagramModel {
  const steps = documents.map((doc, i) => {
    const summary = summaries[i];
    const checks = summary?.running
      ? summary.running.checks
      : doc.asserts.length + (summary?.total_responses ?? 0);
    return {
      index: i + 1,
      request: doc.endpoint,
      response: responseLabel(summary?.error_expected ?? false, checks),
      binds: summary?.running ? summary.running.binds : Object.keys(doc.extracts),
      streaming: doc.kind !== 'unary',
      parallel: doc.parallel === true,
    };
  });
  return { server: documents.find(d => d.address)?.address ?? '', steps };
}

function responseLabel(errorExpected: boolean, checks: number): string {
  const answer = errorExpected ? 'an error' : 'the answer';
  if (checks === 0) return `${answer}, unchecked`;
  return `${answer} · ${checks === 1 ? '1 check' : `${checks} checks`}`;
}

const LANE = { top: 34, gap: 26, request: 22, response: 22, note: 20, bottom: 18 };

export interface StepGeometry {
  y: number;
  request: number;
  response: number;
  note: number | null;
  height: number;
}

export function diagramLayout(model: DiagramModel): { height: number; steps: StepGeometry[] } {
  let y = LANE.top;
  const steps = model.steps.map(step => {
    const request = y + LANE.request;
    const response = request + LANE.response;
    const note = step.binds.length > 0 ? response + LANE.note : null;
    const height = (note ?? response) + LANE.gap - y;
    const geometry = { y, request, response, note, height };
    y += height;
    return geometry;
  });
  return { height: y + LANE.bottom, steps };
}

export function fit(text: string, max = 46): string {
  return text.length <= max ? text : `${text.slice(0, max - 1)}…`;
}
