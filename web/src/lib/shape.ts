import { looksHttp } from './http-endpoint';
import type { ReflectionMethod } from './types';

export type Shape = 'unary' | 'server' | 'client' | 'bidi';

export const SHAPE_TONE: Record<Shape, string> = {
  unary: 'kind-simple',
  server: 'kind-down',
  client: 'kind-up',
  bidi: 'kind-duplex',
};

export const SHAPE_LABEL: Record<Shape, string> = {
  unary: 'unary',
  server: 'server',
  client: 'client',
  bidi: 'bidi',
};

export const SHAPE_ARROW: Record<Shape, string> = {
  unary: '→',
  server: '↓',
  client: '↑',
  bidi: '↕',
};

export function stepShape(step: { kind: Shape; endpoint: string }): {
  label: string;
  tone: string;
} {
  if (looksHttp(step.endpoint)) return { label: 'http', tone: 'kind-down' };
  return { label: SHAPE_LABEL[step.kind], tone: SHAPE_TONE[step.kind] };
}

export function shapeOfMethod(m: Pick<ReflectionMethod, 'clientStreaming' | 'serverStreaming'>): Shape {
  if (m.clientStreaming && m.serverStreaming) return 'bidi';
  if (m.clientStreaming) return 'client';
  if (m.serverStreaming) return 'server';
  return 'unary';
}

export function shapeOfName(name: string | null | undefined): Shape | null {
  if (name === 'unary' || name === 'server' || name === 'client') return name;
  if (name === 'duplex') return 'bidi';
  return null;
}

export function shapeOfRequest(
  endpoint: string,
  bodyCount: number,
  methods: ReflectionMethod[],
  responseMessages = 0,
  reported?: string | null,
): Shape {
  const method = methods.find(m => m.fullName === endpoint);
  if (method) return shapeOfMethod(method);
  const said = shapeOfName(reported);
  if (said) return said;
  const sent = bodyCount > 1;
  const received = responseMessages > 1;
  if (sent && received) return 'bidi';
  if (sent) return 'client';
  if (received) return 'server';
  return 'unary';
}

export function shapeSource(
  endpoint: string,
  methods: ReflectionMethod[],
  reported?: string | null,
): 'schema' | 'call' | 'guess' {
  if (methods.some(m => m.fullName === endpoint)) return 'schema';
  if (shapeOfName(reported)) return 'call';
  return 'guess';
}

export const SHAPE_SOURCE_NOTE: Record<'schema' | 'call' | 'guess', string> = {
  schema: 'From the schema the server reflected',
  call: 'From what the last call resolved on the target',
  guess: 'Guessed from the message count — no schema loaded and nothing called yet',
};

export function shapeMismatch(
  endpoint: string,
  bodyCount: number,
  methods: ReflectionMethod[],
  reported?: string | null,
): boolean {
  const method = methods.find(m => m.fullName === endpoint);
  const clientStreams = method ? method.clientStreaming : shapeOfName(reported) === 'client' || shapeOfName(reported) === 'bidi';
  const known = !!method || shapeOfName(reported) !== null;
  return known && !clientStreams && bodyCount > 1;
}
