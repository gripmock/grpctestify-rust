export type AssertWhy = {
  expected: string | null;
  actual: string | null;
  message: string | null;
  hint: string | null;
};

export function isBlock(why: AssertWhy | null): boolean {
  if (!why) return false;
  return (why.expected?.includes('\n') ?? false) || (why.actual?.includes('\n') ?? false);
}

export function assertWhy(a: {
  expected?: string | null;
  actual?: string | null;
  message?: string | null;
  hint?: string | null;
  expression?: string | null;
}): AssertWhy | null {
  const expected = trimmed(a.expected);
  const actual = trimmed(a.actual);
  const message = withoutExpression(trimmed(a.message), trimmed(a.expression));
  const hint = trimmed(a.hint);

  if (expected !== null || actual !== null) {
    return { expected: withoutEquals(expected), actual, message: null, hint };
  }
  return message === null && hint === null
    ? null
    : { expected: null, actual: null, message, hint };
}

function trimmed(value: string | null | undefined): string | null {
  const text = (value ?? '').trim();
  return text === '' ? null : text;
}

function withoutExpression(message: string | null, expression: string | null): string | null {
  if (message === null || expression === null) return message;
  return message.endsWith(`: ${expression}`)
    ? trimmed(message.slice(0, -(expression.length + 2)))
    : message;
}

export function takeApart(expression: string): boolean {
  const expr = expression.trim();
  return expr.startsWith('.') && !expr.includes('@') && !expr.includes('\n');
}

function withoutEquals(expected: string | null): string | null {
  if (expected === null) return null;
  const equality = expected.match(/^==\s+(.+)$/s);
  return equality ? equality[1].trim() : expected;
}

export interface StepRange {
  index: number;
  endpoint: string;
  start_line?: number;
  end_line?: number;
}

export function stepOfLine(steps: StepRange[], line: number): StepRange | null {
  for (const step of steps) {
    if (step.start_line === undefined || step.end_line === undefined) continue;
    if (line > step.start_line && line <= step.end_line) return step;
  }
  return null;
}

export function groupByStep<T extends { line: number }>(
  checks: T[],
  steps: StepRange[],
): { step: StepRange | null; checks: T[] }[] {
  const out: { step: StepRange | null; checks: T[] }[] = [];
  for (const check of checks) {
    const step = steps.length > 1 ? stepOfLine(steps, check.line) : null;
    const last = out[out.length - 1];
    if (last && last.step === step) last.checks.push(check);
    else out.push({ step, checks: [check] });
  }
  return out;
}

export function stepHeading(
  written: string,
  checks: { endpoint?: string | null }[],
): string {
  const named = new Set(
    checks.map(c => c.endpoint?.trim()).filter((e): e is string => !!e && e !== ''),
  );
  return named.size === 1 ? [...named][0] : written;
}

export function stepPhrase(steps: number, activeStep: number): string {
  return steps > 1 ? `step ${activeStep + 1}` : '';
}
