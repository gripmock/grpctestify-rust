export type RunGateChoice = 'save' | 'run' | null;

export type RunGateOutcome = 'ran' | 'saved-and-ran' | 'save-refused' | 'cancelled';

export async function runWithGate(
  choice: RunGateChoice,
  actions: { save: () => Promise<boolean>; run: () => Promise<void> },
): Promise<RunGateOutcome> {
  if (choice === null) return 'cancelled';
  if (choice === 'save') {
    if (!(await actions.save())) return 'save-refused';
    await actions.run();
    return 'saved-and-ran';
  }
  await actions.run();
  return 'ran';
}
