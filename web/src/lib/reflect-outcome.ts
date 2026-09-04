export interface ReflectAttempt {
  aborted: boolean;
  timedOut: boolean;
  superseded: boolean;
  hadMethods: boolean;
  transportError?: string | null;
  ok?: boolean;
  status?: number;
  statusText?: string;
  reported?: string | null;
  methodCount?: number;
  seconds?: number;
}

export interface ReflectResolution {
  status: 'idle' | 'loading' | 'ok' | 'error';
  error: string | null;
  clearMethods: boolean;
}

export function reflectOutcome(attempt: ReflectAttempt): ReflectResolution {
  const {
    aborted, timedOut, superseded, hadMethods,
    transportError, ok, status, statusText, reported, methodCount = 0, seconds = 30,
  } = attempt;

  if (superseded) return { status: 'loading', error: null, clearMethods: false };

  if (aborted && !timedOut) {
    return { status: hadMethods ? 'ok' : 'idle', error: null, clearMethods: false };
  }

  if (timedOut) {
    return { status: 'error', error: `no answer in ${seconds} s`, clearMethods: true };
  }

  if (transportError) return { status: 'error', error: transportError, clearMethods: true };

  if (ok === false) {
    const said = [status, statusText].filter(Boolean).join(' ').trim();
    return { status: 'error', error: said ? `the server answered ${said}` : 'the server refused', clearMethods: true };
  }

  if (reported) return { status: 'error', error: reported, clearMethods: true };

  if (methodCount === 0) {
    return {
      status: 'error',
      error: 'the server reflected no methods — it may not serve reflection; a PROTO descriptor in the file works without it',
      clearMethods: true,
    };
  }

  return { status: 'ok', error: null, clearMethods: false };
}

export function schemaKey(input: { address: string; protocol: string; collectionPath?: string | null }): string {
  return `${input.address.trim()}|${input.protocol}|${input.collectionPath ?? ''}`;
}

export function shouldAskServer(input: {
  address: string;
  askedFor: string | null;
  status: 'idle' | 'loading' | 'ok' | 'error';
  key: string;
}): boolean {
  if (input.address.trim() === '') return false;
  if (input.status === 'loading') return false;
  return input.askedFor !== input.key;
}
