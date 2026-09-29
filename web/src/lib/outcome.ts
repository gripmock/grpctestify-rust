import { count } from 'luvo/data/plural';
export interface OutcomeBadge {
  kind: 'pending' | 'ok' | 'checks' | 'error' | 'refused';
  label: string;
  title?: string;
}

export function outcomeBadge(response: {
  status: 'ok' | 'error' | 'pending';
  messages: unknown[];
  statusCode?: number | null;
  assertions?: { passed: boolean }[] | null;
  fromRun?: boolean;
  sent?: boolean;
}, isHttp = false): OutcomeBadge {
  if (response.status === 'pending') return { kind: 'pending', label: 'executing' };

  if (response.sent === false) {
    return {
      kind: 'refused',
      label: 'not sent',
      title: 'The request was not dialled — what is below says what to fix',
    };
  }

  const checks = response.assertions ?? [];
  const passed = checks.filter(c => c.passed).length;

  if (response.status === 'ok') {
    if (isHttp && (response.statusCode ?? 0) >= 400) {
      return {
        kind: 'error',
        label: 'answered',
        title: 'The request reached the server; the status it answered with is a failure',
      };
    }
    if (isHttp) return { kind: 'ok', label: 'ok' };
    if (response.fromRun && response.messages.length === 0) {
      return {
        kind: 'ok',
        label: 'ok',
        title: 'The run kept this file’s checks and not its answer — a passing run keeps no body',
      };
    }
    const msgs = `${count(response.messages.length, 'msg')}`;
    return { kind: 'ok', label: `ok · ${msgs}` };
  }

  if (checks.length > 0 && passed < checks.length && response.messages.length > 0) {
    return {
      kind: 'checks',
      label: `checks failed · ${passed}/${checks.length}`,
      title: 'The call answered; what came back is not what this file expects',
    };
  }

  if (!isHttp && response.messages.length > 0) {
    const msgs = `${count(response.messages.length, 'msg')}`;
    return {
      kind: 'error',
      label: `error · ${msgs}`,
      title: `${msgs} came back before the call failed — they are below`,
    };
  }

  return { kind: 'error', label: 'error' };
}

export function metaEmptyNote(isHttp: boolean, fromRun: boolean): string {
  const what = isHttp ? 'headers' : 'metadata';
  return fromRun
    ? `The run kept this file's checks, not its answer — Execute to see the ${what}.`
    : `The server sent no ${what} with this response.`;
}
