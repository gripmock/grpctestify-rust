import { describe, it, expect } from 'vitest';
import { timeoutSeconds, grpcStatusLabel, byteSize, humanBytes, jsonProblem, jsonStream, shortPath, httpStatusLabel, httpStatusTone, sentHeaders, bodyLanguage, capLines } from './format';

describe('grpcStatusLabel', () => {
  it('names the codes people actually see', () => {
    expect(grpcStatusLabel(0)).toBe('0 OK');
    expect(grpcStatusLabel(14)).toBe('14 UNAVAILABLE');
    expect(grpcStatusLabel(4)).toBe('4 DEADLINE_EXCEEDED');
  });

  it('passes an unknown code through rather than inventing a name', () => {
    expect(grpcStatusLabel(99)).toBe('99');
  });

  it('is absent when there is no status', () => {
    expect(grpcStatusLabel(null)).toBeNull();
  });
});

describe('byteSize', () => {
  it('measures the encoded payload, not the character count', () => {
    expect(byteSize({ a: 1 })).toBe(7);
    expect(byteSize('привет')).toBe(12);
    expect(byteSize(undefined)).toBe(0);
  });
});

describe('humanBytes', () => {
  it('scales by unit', () => {
    expect(humanBytes(412)).toBe('412 B');
    expect(humanBytes(2048)).toBe('2.0 kB');
    expect(humanBytes(3 * 1024 * 1024)).toBe('3.0 MB');
  });
});

describe('jsonProblem', () => {
  it('is nothing for valid JSON and for an empty body', () => {
    expect(jsonProblem('{"a":1}')).toBeNull();
    expect(jsonProblem('')).toBeNull();
    expect(jsonProblem('   \n')).toBeNull();
  });

  it('says what is wrong, without the position tail', () => {
    const problem = jsonProblem('{"a":}');
    expect(problem).toBeTruthy();
    expect(problem).not.toContain('position');
  });

  it('drops the byte offset but keeps the line and column', () => {
    const problem = jsonProblem('{}z');
    expect(problem).not.toMatch(/position \d/);
    expect(problem).toMatch(/line 1 column 3/);
  });

  it('treats a bare word as the problem it is', () => {
    expect(jsonProblem('nope')).toBeTruthy();
  });
});

describe('shortPath', () => {
  it('leaves a path a row can hold', () => {
    expect(shortPath('./schema.desc')).toBe('./schema.desc');
  });

  it('keeps the end, which is what tells two files apart', () => {
    const long = '/Users/someone/go/src/github.com/org/project/target/debug/build/out/test_servers.bin';
    const short = shortPath(long);
    expect(short.startsWith('…')).toBe(true);
    expect(short.endsWith('test_servers.bin')).toBe(true);
    expect(short.length).toBeLessThanOrEqual(36);
  });

  it('cuts at a separator rather than mid-name', () => {
    expect(shortPath('/a/very/long/prefix/that/keeps/going/and/going/file.bin')).toContain('/file.bin');
  });
});

describe('httpStatusLabel', () => {
  it('names the codes anyone reads on a screen', () => {
    expect(httpStatusLabel(200)).toBe('200 OK');
    expect(httpStatusLabel(404)).toBe('404 Not Found');
    expect(httpStatusLabel(418)).toBe("418 I'm a teapot");
  });

  it('shows a code it has no name for as the number it is', () => {
    expect(httpStatusLabel(599)).toBe('599');
    expect(httpStatusLabel(null)).toBeNull();
  });

  it('tells the three families of status apart', () => {
    expect(httpStatusTone(204)).toBe('ok');
    expect(httpStatusTone(301)).toBe('warn');
    expect(httpStatusTone(500)).toBe('fail');
    expect(httpStatusTone(null)).toBeNull();
  });
});

describe('the headers a response shows', () => {
  it('leaves out the pseudo-headers the protocol adds', () => {
    expect(sentHeaders({ ':status': '200', 'content-type': 'application/json' }))
      .toEqual({ 'content-type': 'application/json' });
  });

  it('keeps everything a person would recognise', () => {
    const real = { 'x-request-id': 'r-1', server: 'nginx' };
    expect(sentHeaders(real)).toEqual(real);
    expect(sentHeaders({})).toEqual({});
  });
});

describe('the language a response is shown in', () => {
  it('is JSON whenever the body parsed as JSON, whatever was declared', () => {
    expect(bodyLanguage({ 'content-type': 'text/plain' }, true)).toBe('json');
  });

  it('is what the server said it sent', () => {
    expect(bodyLanguage({ 'content-type': 'text/html; charset=utf-8' }, false)).toBe('html');
    expect(bodyLanguage({ 'Content-Type': 'application/xml' }, false)).toBe('xml');
    expect(bodyLanguage({ 'content-type': 'application/problem+json' }, false)).toBe('json');
  });

  it('is plain text when nothing says otherwise', () => {
    expect(bodyLanguage({}, false)).toBe('plaintext');
    expect(bodyLanguage({ 'content-type': 'application/octet-stream' }, false)).toBe('plaintext');
  });
});

describe('a body of several JSON messages', () => {
  it('counts them', () => {
    expect(jsonStream('{"n":1}\n{"n":2}\n{"n":3}')).toEqual({ messages: 3, problem: null });
    expect(jsonStream('{"n":1}')).toEqual({ messages: 1, problem: null });
    expect(jsonStream('  ')).toEqual({ messages: 0, problem: null });
  });

  it('reads messages written across lines', () => {
    expect(jsonStream('{\n  "n": 1\n}\n{\n  "n": 2\n}')).toEqual({ messages: 2, problem: null });
  });

  it('still says what is wrong with the last one', () => {
    const { messages, problem } = jsonStream('{"n":1}\n{"n":');
    expect(messages).toBe(1);
    expect(problem).not.toBeNull();
  });
});

describe('timeoutSeconds', () => {
  it('rounds up to the second the call is sent with', () => {
    expect(timeoutSeconds(1500)).toBe(2);
    expect(timeoutSeconds(30_000)).toBe(30);
    expect(timeoutSeconds(1)).toBe(1);
  });

  it('keeps "no timeout" as no timeout', () => {
    expect(timeoutSeconds(0)).toBe(0);
  });
});

describe('a refusal too long to print', () => {
  it('keeps the first lines and counts the rest', () => {
    const text = Array.from({ length: 10 }, (_, i) => `line ${i + 1}`).join('\n');
    expect(capLines(text, 4)).toEqual({ shown: 'line 1\nline 2\nline 3\nline 4', hidden: 6 });
  });

  it('leaves a short one alone', () => {
    expect(capLines('one\ntwo\n', 4)).toEqual({ shown: 'one\ntwo', hidden: 0 });
  });
});
