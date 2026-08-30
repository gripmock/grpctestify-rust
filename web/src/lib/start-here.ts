import { count } from 'luvo/data/plural';

export interface StartHint {
  title: string;
  hint: string | null;
  ready: boolean;
  ask: boolean;
  bring: boolean;
}

export function startHint(input: {
  endpoint: string;
  methodCount: number;
  fileCount: number;
  hasWorkspaceFile: boolean;
  address: string;
  reflectionRefused?: boolean;
}): StartHint {
  const ask = input.endpoint.trim() === ''
    && input.methodCount === 0
    && input.address.trim() !== ''
    && !input.reflectionRefused;

  const bring = input.endpoint.trim() === '' && input.methodCount === 0;

  if (input.endpoint.trim() !== '') {
    return { title: 'no response yet', hint: null, ready: true, ask: false, bring: false };
  }

  if (input.methodCount > 0) {
    return {
      title: 'pick a method',
      hint: `${input.methodCount} came back from ${input.address || 'the target'} — the endpoint field lists them.`,
      ready: false,
      ask,
      bring,
    };
  }

  if (input.reflectionRefused) {
    return {
      title: 'the server did not say what it serves',
      hint: `${input.address} does not answer reflection — type the method, or name a PROTO descriptor in config.`,
      ready: false,
      ask: false,
      bring,
    };
  }

  if (input.hasWorkspaceFile) {
    return {
      title: 'this file names no endpoint',
      hint: 'Type one, or pick it from the list once the target answers reflection.',
      ready: false,
      ask,
      bring: false,
    };
  }

  if (input.fileCount > 0) {
    return {
      title: 'nothing to send yet',
      hint: `Open a file from the rail, or type an endpoint${input.address ? ` for ${input.address}` : ''}.`,
      ready: false,
      ask,
      bring,
    };
  }

  return {
    title: 'nothing to send yet',
    hint: 'Type an endpoint, or drop a file anywhere in the window.',
    ready: false,
    ask,
    bring,
  };
}

export interface StartStep {
  key: 'target' | 'method' | 'send';
  label: string;
  detail: string;
  done: boolean;
}

export function startSteps(input: {
  endpoint: string;
  methodCount: number;
  address: string;
  reachable: boolean | null;
  defaulted?: boolean;
}): StartStep[] {
  const address = input.address.trim();
  const method = input.endpoint.trim();
  return [
    {
      key: 'target',
      label: 'point at a server',
      detail: address === ''
        ? 'no address yet'
        : input.reachable === false
          ? `${address} — nothing answered there`
          : input.defaulted
            ? `${address} — where a gRPC call goes when nothing else names one`
            : address,
      done: address !== '' && input.reachable !== false,
    },
    {
      key: 'method',
      label: 'choose what to call',
      detail: method !== ''
        ? method
        : input.methodCount > 0
          ? `${count(input.methodCount, 'method')} to pick from`
          : 'ask the server what it serves, or type it — a gRPC method, or GET /a/path',
      done: method !== '',
    },
    {
      key: 'send',
      label: 'send it',
      detail: method !== '' ? 'the answer lands here' : 'nothing to send yet',
      done: false,
    },
  ];
}
