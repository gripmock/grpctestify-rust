import { errorText } from './grpc-error';

export interface Failure {
  title: string;
  detail: string | null;
  fixes: string[];
}

export function explainFailure(
  error: string,
  statusCode?: number | null,
  dialled?: string | null,
  serves?: string[],
): Failure {
  const text = errorText(error).trim();

  const absent = /^(Service|Method) '([^']+)' not found$/.exec(text);
  if (absent) {
    const [, kind, name] = absent;
    const where = dialled?.trim();
    const known = [...new Set(serves ?? [])].sort();
    return {
      title: where ? `${name} is not on ${where}` : `${name} is not on this target`,
      detail: null,
      fixes: [
        known.length > 0
          ? `${where || 'It'} serves ${known.join(' · ')}`
          : 'Open the endpoint field — it asks the target what it serves',
        kind === 'Method'
          ? 'Check the method name, or pick one from the endpoint field'
          : 'Check the address beside the endpoint — the usual cause is the right file aimed at the wrong target',
        'If the target has no reflection, name `PROTO descriptor:` or `PROTO files:` in the file',
      ],
    };
  }

  const unreachable = /^Could not reach (\S+): (.*)$/s.exec(text);
  if (unreachable) {
    const [, address, why] = unreachable;
    return {
      title: `Could not reach ${address}`,
      detail: why,
      fixes: reachFixes(why),
    };
  }

  if (/invalid compression flag|frame with invalid size|invalid gRPC frame|http\/1|internal protocol error/i.test(text)) {
    const at = /(?:Reflection failed at|Could not reach|at) ([^\s:]+:\d+)/.exec(text)?.[1]
      ?? (dialled?.trim() || undefined);
    return {
      title: at ? `${at} answered, but not as gRPC` : 'The target answered, but not as gRPC',
      detail: text,
      fixes: [
        'A plain HTTP port often looks like this — check the port',
        'If the server speaks gRPC-Web or Connect, choose that transport beside the address',
        'If it is an HTTP API, the test belongs in a `.httf` — a method and a path',
      ],
    };
  }

  const silent = /^(.+?) did not answer: (.*)$/s.exec(text);
  if (silent) {
    const [, url, why] = silent;
    const notHttp = /invalid http version|invalid http response|connection closed before message completed|unexpected end of file/i
      .test(why);
    return {
      title: notHttp ? `${url} answered, but not as HTTP` : `${url} did not answer`,
      detail: why,
      fixes: notHttp
        ? [
          'A gRPC port looks like this — it speaks HTTP/2 and never sends an HTTP/1 answer',
          'Check the port beside the path',
          'If the target is gRPC, the test belongs in a `.gctf` — a service and a method',
        ]
        : [],
    };
  }

  if (/does not serve reflection/i.test(text)) {
    return {
      title: text,
      detail: null,
      fixes: [
        'Start the server with the gRPC reflection service',
        'Or name `PROTO descriptor:` or `PROTO files:` in the file — the schema then comes from disk',
      ],
    };
  }

  if (/answered reflection with no files/i.test(text)) {
    return {
      title: text,
      detail: null,
      fixes: ['Name `PROTO descriptor:` or `PROTO files:` in the file to work without reflection'],
    };
  }

  return { title: text, detail: null, fixes: codeFixes(statusCode ?? null) };
}

function reachFixes(why: string): string[] {
  if (/connection refused/i.test(why)) {
    return ['Nothing is listening there — start the server, or check the port'];
  }
  if (/lookup address|not known|no such host|dns/i.test(why)) {
    return ['The host is unknown — check the spelling, and that it resolves from here'];
  }
  if (/frame with invalid size|invalid frame|http\/1|protocol error/i.test(why)) {
    return [
      'Something answered, but not as gRPC — a plain HTTP port often looks like this',
      'If the server speaks gRPC-Web or Connect, say so in the transport beside the address',
    ];
  }
  if (/certificate|handshake|tls|ssl/i.test(why)) {
    return ['The certificate was refused — set the CA in TLS, or allow an insecure connection there'];
  }
  if (/timed out|timeout|deadline/i.test(why)) {
    return ['The address answered too slowly — check the target, or raise `timeout` in OPTIONS'];
  }
  return [];
}

function codeFixes(code: number | null): string[] {
  switch (code) {
    case 3: return ['The server rejected the message — check its fields against the schema'];
    case 4: return ['The server ran past the deadline — raise `timeout` in OPTIONS if it needs longer'];
    case 12: return ['That method is not on this server — check the endpoint, and that you are pointed at the right target'];
    case 14: return ['The server is not taking calls — check it is up, and that the address is the one it serves'];
    case 16: return ['Add credentials in REQUEST_HEADERS'];
    default: return [];
  }
}

export function unresolvedNames(message: string): string[] {
  if (!message.includes('Unresolved variable placeholder')) return [];
  const names: string[] = [];
  for (const m of message.matchAll(/\{\{\s*([A-Za-z_][A-Za-z0-9_.]*)\s*\}\}/g)) {
    if (!names.includes(m[1])) names.push(m[1]);
  }
  return names;
}
