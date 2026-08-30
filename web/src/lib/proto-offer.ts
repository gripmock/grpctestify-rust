export function offerNote(what: 'descriptor set' | '.proto', named: boolean): string {
  return named
    ? `Nothing else in this project — this file names ${what === '.proto' ? 'its own' : 'one of its own'}.`
    : `No ${what} in this project yet — upload one, or drop it anywhere in the window.`;
}
