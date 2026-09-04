import type { ProtoSource } from './section-model';
import { count, plural } from 'luvo/data/plural';

export type SchemaState = {
  kind: 'inspecting' | 'reflected' | 'files' | 'error' | 'unasked' | 'none';
  label: string;
  hint: string;
  tone: 'ok' | 'fail' | 'muted' | 'pending';
};

export function schemaState(input: {
  reflectStatus: 'idle' | 'loading' | 'ok' | 'error';
  reflectError: string | null;
  methodCount: number;
  serviceCount?: number;
  protoSource: ProtoSource;
  protoFiles: number;
  protoNames?: string;
  reflectedAt?: string | null;
}): SchemaState {
  const {
    reflectStatus, reflectError, methodCount, serviceCount = 0,
    protoSource, protoFiles, protoNames = '', reflectedAt = null,
  } = input;

  if (reflectStatus === 'loading') {
    return { kind: 'inspecting', label: 'inspecting schema', hint: 'Asking the server what it serves', tone: 'pending' };
  }

  if (protoSource === 'descriptor' || protoSource === 'files') {
    const what = protoSource === 'descriptor'
      ? 'descriptor set'
      : `${protoFiles} proto ${plural(protoFiles, 'file')}`;
    const named = fileNames(protoNames);
    return {
      kind: 'files',
      label: named ? `${what} · ${named}` : what,
      hint: 'The file carries its own schema — reflection is not used',
      tone: 'ok',
    };
  }

  if (reflectStatus === 'ok' && methodCount > 0) {
    const services = serviceCount > 0 ? ` · ${count(serviceCount, 'service')}` : '';
    return {
      kind: 'reflected',
      label: `${count(methodCount, 'method')}${services}`,
      hint: reflectedAt
        ? `Reflection answered at ${reflectedAt} — the method list is the server’s own, as it was then`
        : 'Reflection answered — the method list is the server’s own',
      tone: 'ok',
    };
  }

  if (reflectStatus === 'error') {
    const said = plainReason(reflectError);
    const carriesAdvice = /proto/i.test(said);
    const namesReflection = /\bno reflection\b/i.test(said);
    return {
      kind: 'error',
      label: 'no schema',
      hint: !said ? 'Reflection failed. Point the file at a .proto instead.'
        : namesReflection ? said
        : carriesAdvice ? `Reflection failed — ${said}`
        : `Reflection failed — ${said}. Point the file at a .proto instead.`,
      tone: 'fail',
    };
  }

  if (reflectStatus === 'idle') {
    return {
      kind: 'unasked',
      label: 'not asked yet',
      tone: 'muted',
      hint: 'Nothing has asked this server what it serves — ask it, or name a PROTO section',
    };
  }

  return {
    kind: 'none',
    label: 'no methods',
    hint: 'Reflection answered with nothing — this server serves no methods it will name',
    tone: 'muted',
  };
}

export function fileNames(paths: string): string {
  const names = paths
    .split(',')
    .map(p => p.trim().split('/').pop() ?? '')
    .filter(Boolean);
  if (names.length === 0) return '';
  if (names.length <= 2) return names.join(', ');
  return `${names[0]} and ${names.length - 1} more`;
}

export function plainReason(error: string | null): string {
  const said = (error ?? '').trim().replace(/^reflection failed:?\s*/i, '').trim();
  return said.replace(/[.\s]+$/, '');
}
