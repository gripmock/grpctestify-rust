import { describe, it, expect } from 'vitest';
import { addressDecision, addressPlaceholder, chainAddressAt, chainAddressSource, checkAddress, effectiveAddress, runDivergence } from './address';
import { addressForSave } from './save-meta';

const ok = (s: string) => expect(checkAddress(s).ok, s).toBe(true);
const bad = (s: string, part: string) => {
  const r = checkAddress(s);
  expect(r.ok, s).toBe(false);
  expect(r.reason).toContain(part);
};

describe('grading an address before it is dialled', () => {
  it('accepts what the transport dials', () => {
    ok('localhost:4770');
    ok('127.0.0.1:50051');
    ok('api.example.com:443');
    ok('[::1]:50051');
    ok('http://gateway:8080');
    ok('https://api.example.com');
    ok('https://api.example.com/prefix');
  });

  it('leaves a template to the environment', () => {
    ok('{{HOST}}:{{PORT}}');
    ok('{{TARGET}}');
  });

  it('names what is wrong', () => {
    expect(checkAddress('')).toEqual({ ok: true });
    expect(checkAddress('   ')).toEqual({ ok: true });
    bad('not a host', 'no spaces');
    bad('localhost', 'No port');
    bad('localhost:abc', 'not a port');
    bad('localhost:0', 'between 1 and 65535');
    bad('localhost:70000', 'between 1 and 65535');
    bad('::1:50051', 'brackets');
    bad('[::1:50051', 'closing bracket');
    bad('host!:80', 'not a host name');
    bad('http://', 'No host after the scheme');
  });
});

describe('effectiveAddress', () => {
  it('is what was typed when the file names no address', () => {
    expect(effectiveAddress('localhost:4770', null)).toEqual({
      address: 'localhost:4770', source: 'client', overridden: false,
    });
    expect(effectiveAddress('localhost:4770', '   ')).toEqual({
      address: 'localhost:4770', source: 'client', overridden: false,
    });
  });

  it('is the file when the file names one', () => {
    expect(effectiveAddress('localhost:4770', 'prod:443')).toEqual({
      address: 'prod:443', source: 'file', overridden: true,
    });
  });

  it('is not an override when the two agree', () => {
    expect(effectiveAddress(' prod:443 ', 'prod:443')).toEqual({
      address: 'prod:443', source: 'file', overridden: false,
    });
  });

  it('is not an override when nothing was typed at all', () => {
    expect(effectiveAddress('', 'prod:443')).toEqual({
      address: 'prod:443', source: 'file', overridden: false,
    });
  });
});

describe('where a call goes', () => {
  const base = { typed: '', environment: '', server: '', fallback: 'localhost:4770' };

  it('takes the file first', () => {
    expect(addressDecision({ ...base, file: 'prod:443', typed: 'staging:443', environment: 'e:1', server: 's:1' }))
      .toMatchObject({ address: 'prod:443', source: 'file' });
  });

  it('then what was typed', () => {
    expect(addressDecision({ ...base, typed: 'staging:443', environment: 'e:1', server: 's:1' }))
      .toMatchObject({ address: 'staging:443', source: 'typed' });
  });

  it('then the active environment', () => {
    expect(addressDecision({ ...base, environment: 'e:1', server: 's:1' }))
      .toMatchObject({ address: 'e:1', source: 'environment' });
  });

  it('then the environment the server was started in', () => {
    expect(addressDecision({ ...base, server: 's:1' })).toMatchObject({ address: 's:1', source: 'server' });
  });

  it('and the transport default last', () => {
    expect(addressDecision(base)).toMatchObject({ address: 'localhost:4770', source: 'default' });
  });

  it('ignores blanks at every step', () => {
    expect(addressDecision({ file: ' ', typed: ' ', environment: ' ', server: ' ', fallback: 'd:1' }).source)
      .toBe('default');
  });
});

describe('what the address field shows when nothing was typed', () => {
  it('names the transport default, which is not the same for all of them', () => {
    expect(addressPlaceholder({ protocol: 'grpc' })).toBe('localhost:4770');
    expect(addressPlaceholder({ protocol: 'grpc-web' })).toBe('localhost:4769');
    expect(addressPlaceholder({ protocol: 'connectrpc' })).toBe('localhost:4769');
  });

  it('prefers the file, then the environment, then how the server was started', () => {
    expect(addressPlaceholder({ file: 'file:1', environment: 'env:2', server: 'srv:3', protocol: 'grpc' })).toBe('file:1');
    expect(addressPlaceholder({ environment: 'env:2', server: 'srv:3', protocol: 'grpc' })).toBe('env:2');
    expect(addressPlaceholder({ server: 'srv:3', protocol: 'grpc' })).toBe('srv:3');
  });
});

describe('what a placeholder never does', () => {
  it('is not written into a file that was saved without typing an address', () => {
    expect(addressForSave(null, '', false)).toBeUndefined();
    expect(addressForSave(null, '   ', false)).toBeUndefined();
  });

  it('leaves an address a file already has exactly as it was', () => {
    expect(addressForSave({ address: 'pinned:9000' }, '', false)).toBe('pinned:9000');
    expect(addressForSave({ address: 'pinned:9000' }, 'localhost:4770', false)).toBe('pinned:9000');
  });
});

describe('an address in an HTTP file', () => {
  it('needs no port', () => {
    expect(checkAddress('api.example.com', 'httf').ok).toBe(true);
    expect(checkAddress('https://api.example.com', 'httf').ok).toBe(true);
    expect(checkAddress('api.example.com').ok).toBe(false);
  });

  it('is still an address', () => {
    expect(checkAddress('api example.com', 'httf').ok).toBe(false);
    expect(checkAddress('host:not-a-port', 'httf').ok).toBe(false);
  });

  it('is hinted at by shape rather than by a gRPC port', () => {
    expect(addressPlaceholder({ protocol: 'grpc', family: 'httf' })).toBe('https://api.example.com');
    expect(addressPlaceholder({ protocol: 'grpc' })).toBe('localhost:4770');
    expect(addressPlaceholder({ file: 'http://127.0.0.1:8899', protocol: 'grpc', family: 'httf' }))
      .toBe('http://127.0.0.1:8899');
  });

  it('has nowhere to go when nothing names one', () => {
    const decision = addressDecision({ typed: '', fallback: '' });
    expect(decision.address).toBe('');
    expect(decision.why).toContain('needs an address');
  });
});

describe('the address a chain step dials', () => {
  const step = (address: string) => ({
    address,
    address_source: (address === '' ? 'inherited' : 'section') as 'section' | 'inherited',
  });

  it('is the nearest ADDRESS above it', () => {
    const steps = [step('http://api:8899'), step(''), step('')];
    expect(chainAddressAt(steps, 0)).toBe('http://api:8899');
    expect(chainAddressAt(steps, 2)).toBe('http://api:8899');
  });

  it('changes where a later step declares its own', () => {
    const steps = [step('one:1'), step('two:2'), step('')];
    expect(chainAddressAt(steps, 2)).toBe('two:2');
    expect(chainAddressAt(steps, 1)).toBe('two:2');
  });

  it('skips the steps of the other transport', () => {
    const steps = [
      { ...step('http://gate:8899'), endpoint: 'GET /a' },
      { ...step('127.0.0.1:4770'), endpoint: 'pkg.Svc/M' },
      { ...step(''), endpoint: 'GET /b' },
      { ...step(''), endpoint: 'pkg.Svc/N' },
    ];
    expect(chainAddressAt(steps, 2)).toBe('http://gate:8899');
    expect(chainAddressAt(steps, 3)).toBe('127.0.0.1:4770');
  });

  it('says which step named the address', () => {
    const steps = [
      { ...step('one:1'), endpoint: 'a.A/One' },
      { ...step(''), endpoint: 'a.A/Two' },
      { ...step('three:3'), endpoint: 'a.A/Three' },
      { ...step(''), endpoint: 'a.A/Four' },
    ];
    expect(chainAddressSource(steps, 1).from).toBe(0);
    expect(chainAddressSource(steps, 3)).toEqual({ address: 'three:3', from: 2 });
    expect(chainAddressSource([step('')], 0)).toEqual({ address: '', from: -1 });
  });

  it('is nothing when the chain names no target', () => {
    expect(chainAddressAt([step(''), step('')], 1)).toBe('');
    expect(chainAddressAt([], 0)).toBe('');
  });

  it('names the chain, not the file, when the address came from an earlier step', () => {
    const from = (fileFromChain: boolean) =>
      addressDecision({ file: 'http://api:8899', fileFromChain, typed: 'localhost:4770', fallback: 'localhost:4770' }).why;
    expect(from(true)).toBe('the address the chain started with');
    expect(from(false)).toBe('the ADDRESS section of this file');
  });
});

describe('where a run of this file would go', () => {
  const decide = (typed: string, environment: string | null) => addressDecision({
    file: null, typed, environment, server: null, fallback: 'localhost:4770',
  });

  it('is said when the header is the only thing aiming Execute', () => {
    const execute = decide('localhost:9999', 'localhost:50051');
    const run = decide('', 'localhost:50051');
    expect(runDivergence(execute, run, true)).toEqual({
      address: 'localhost:50051', source: 'environment', why: 'the address of the active environment',
    });
  });

  it('is nothing when they agree', () => {
    const both = decide('', 'localhost:50051');
    expect(runDivergence(both, both, true)).toBeNull();
  });

  it('is nothing for a draft — there is no file to run', () => {
    expect(runDivergence(decide('localhost:9999', null), decide('', null), false)).toBeNull();
  });
});

describe('an address a call can be made to and still not the one written', () => {
  it('is dialled, and says what is dropped', () => {
    const said = checkAddress('localhost:4770/api', 'gctf');
    expect(said.ok).toBe(true);
    expect(said.note).toContain('is not dialled');
  });

  it('says nothing about a host and a port alone', () => {
    expect(checkAddress('localhost:4770', 'gctf').note).toBeUndefined();
  });

  it('says nothing about an HTTP address', () => {
    expect(checkAddress('https://api.test/v1', 'httf').note).toBeUndefined();
  });
});
