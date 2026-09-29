import { BENCH_GROUPS } from './bench-model';

export type Scenario = {
  name: string;
  description: string;
  keys: Record<string, string>;
};

export function scenariosOf(
  served: { name: string; description: string; keys: [string, string][] }[],
): Scenario[] {
  return served.map(p => ({
    name: p.name,
    description: p.description,
    keys: Object.fromEntries(p.keys),
  }));
}

export function scenarioKeys(scenarios: Scenario[]): string[] {
  return [...new Set(scenarios.flatMap(s => Object.keys(s.keys)))];
}

export function activeScenario(bench: Record<string, string>, scenarios: Scenario[]): string | null {
  const match = scenarios.find(s =>
    Object.entries(s.keys).every(([k, v]) => (bench[k] ?? '').trim() === v),
  );
  return match?.name ?? null;
}

export function applyScenario(
  bench: Record<string, string>,
  scenario: Scenario,
  scenarios: Scenario[],
): Record<string, string> {
  const shaped = { ...bench };
  for (const key of scenarioKeys(scenarios)) delete shaped[key];
  return { ...shaped, ...scenario.keys };
}

export function benchKeys(): string[] {
  return BENCH_GROUPS.flatMap(g => g.fields.map(f => f.key));
}
