import type { VariableUse } from './types';

export function droppedNames(before: string[], after: string[], uses: VariableUse[]): VariableUse[] {
  const kept = new Set(after.map(name => name.trim()).filter(Boolean));
  const gone = before.map(name => name.trim()).filter(name => name !== '' && !kept.has(name));
  return uses.filter(use => gone.includes(use.name));
}

export function droppedQuestion(dropped: VariableUse[]): string {
  const names = dropped.map(d => `{{${d.name}}}`).join(', ');
  const files = [...new Set(dropped.flatMap(d => d.files))];
  const where = files.length <= 3
    ? files.join(', ')
    : `${files.slice(0, 3).join(', ')} and ${files.length - 3} more`;
  return `${names} ${dropped.length === 1 ? 'is' : 'are'} read by ${where}. This environment will stop defining ${dropped.length === 1 ? 'it' : 'them'}, and those placeholders resolve to nothing.`;
}
