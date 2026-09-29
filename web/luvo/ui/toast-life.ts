export type ToastType = 'success' | 'error' | 'info' | 'warn';

/** How long a toast stays, in milliseconds, or `null` for "until it is
 *  dismissed".
 *
 *  Every toast used to live four seconds, errors included — so the one kind
 *  worth reading twice, or pasting into a ticket, was also the one that left
 *  while it was being read. A refusal stays until it is closed; a confirmation
 *  is over as soon as it is seen. */
export function toastLife(type: ToastType): number | null {
  /* And a warning stays too: it is not a failure — nothing went wrong — but it
     is something the next click depends on having been read, and four seconds
     is not long enough to find out that a credential was left behind. */
  return type === 'error' || type === 'warn' ? null : 4000;
}

/** What a refusal is, as a toast.
 *
 *  A refusal is a fact about the state — the step on screen has edits, this
 *  answer belongs to another step — rather than a failure: nothing was
 *  attempted, and the same click says it again. It fades, where a failure stays
 *  until it is closed. Named here so the callers cannot disagree about it. */
export const REFUSAL_TYPE: ToastType = 'info';

/** What the stack keeps. A loop that fails once per call used to fill the
 *  screen bottom to top; the newest are the ones being read. */
export const MAX_TOASTS = 4;

export function keepLast<T>(items: T[], limit = MAX_TOASTS): T[] {
  return items.length <= limit ? items : items.slice(items.length - limit);
}

/** Whether a message is worth adding to what is already on screen.
 *
 *  A refusal stays until it is dismissed, and a condition that keeps failing —
 *  a workbench that stopped answering, checked every fifteen seconds — said the
 *  same sentence again under the first one. The newest toast is the one being
 *  read; saying it twice adds nothing. */
export function repeatsNewest<T extends { type: ToastType; message: string }>(
  items: T[],
  candidate: { type: ToastType; message: string },
): boolean {
  const newest = items[items.length - 1];
  return !!newest && newest.type === candidate.type && newest.message === candidate.message;
}
