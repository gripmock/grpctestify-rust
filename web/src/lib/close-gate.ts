export type CloseChoice = 'discard' | 'save' | null;

export type CloseOutcome = 'closed' | 'kept' | 'save-refused' | 'named';

export async function closeWithGate(
  choice: CloseChoice,
  tab: { hasPath: boolean },
  actions: {
    close: () => void;
    save: () => Promise<boolean>;
    nameIt: () => void;
  },
): Promise<CloseOutcome> {
  if (choice === null) return 'kept';
  if (choice === 'discard') {
    actions.close();
    return 'closed';
  }
  if (!tab.hasPath) {
    actions.nameIt();
    return 'named';
  }
  if (!(await actions.save())) return 'save-refused';
  actions.close();
  return 'closed';
}
