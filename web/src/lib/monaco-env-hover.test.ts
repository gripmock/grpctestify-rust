import { describe, expect, it } from 'vitest';
import { addEnvDecorations, registerEnvHoverProvider, variableAt } from './monaco-env-hover';

function fakeMonaco() {
  const providers: any[] = [];
  return {
    providers,
    languages: {
      getLanguages: () => [{ id: 'json' }],
      registerHoverProvider: (_lang: string, provider: any) => { providers.push(provider); },
    },
  };
}

function hoverOver(line: string, env: any): string | null {
  const monaco = fakeMonaco();
  registerEnvHoverProvider(monaco, () => env);
  const model = { getLineContent: () => line };
  const at = line.indexOf('{{') + 3;
  const said = monaco.providers[0].provideHover(model, { lineNumber: 1, column: at });
  return said ? said.contents[0].value : null;
}

const env = {
  name: 'example',
  source: 'project' as const,
  variables: { USER: 'Ada', AUTH_TOKEN: 'sk-live-abc123' },
};

describe('what a variable says when it is hovered', () => {
  it('shows an ordinary value', () => {
    expect(hoverOver('{{USER}}', env)).toContain('Ada');
  });

  it('does not print a credential', () => {
    const said = hoverOver('{{AUTH_TOKEN}}', env);
    expect(said).not.toContain('sk-live-abc123');
    expect(said).toContain('••••••');
    expect(said).toContain('example');
  });

  it('says nothing about a name the environment does not hold', () => {
    expect(hoverOver('{{TENANT_ID}}', env)).toBeNull();
  });

  it('answers inside a JSON string, and names the world the value came from', () => {
    const said = hoverOver('  "message": "{{USER}}"', env);
    expect(said).toContain('Ada');
    expect(said).toContain('.env.example');
  });

  it('names a browser environment as this browser\'s own', () => {
    const said = hoverOver('{{USER}}', { ...env, source: 'browser' });
    expect(said).toContain("this browser's own");
  });
});

describe('what the painted braces say', () => {
  function decorate(text: string) {
    const captured: any[] = [];
    const model = {
      getValue: () => text,
      getPositionAt: () => ({ lineNumber: 1, column: 1 }),
    };
    const editor = {
      getModel: () => model,
      createDecorationsCollection: (d: any[]) => { captured.push(...d); return { set: () => {} }; },
      onDidChangeModelContent: () => {},
    };
    const monaco = { Range: class { constructor() {} } };
    addEnvDecorations(editor, monaco, () => env);
    return captured;
  }

  it('leaves a known variable to the hover provider', () => {
    const painted = decorate('{ "a": "{{AUTH_TOKEN}}" }')[0];
    expect(painted.options.inlineClassName).toBe('env-var-active');
    expect(painted.options.hoverMessage).toBeUndefined();
  });

  it('still answers for a name nothing holds', () => {
    const painted = decorate('{ "a": "{{TENANT_ID}}" }')[0];
    expect(painted.options.inlineClassName).toBe('env-var-unknown');
    expect(painted.options.hoverMessage.value).toContain('unknown variable');
  });
});

describe('the variable under the pointer', () => {
  const line = '{ "a": "{{ONE}}", "b": "{{TWO}}" }';

  it('finds the one it is inside', () => {
    expect(variableAt(line, line.indexOf('{{ONE') + 3)?.key).toBe('ONE');
    expect(variableAt(line, line.indexOf('{{TWO') + 3)?.key).toBe('TWO');
  });

  it('finds none between them', () => {
    expect(variableAt(line, line.indexOf('", "b') + 2)).toBeNull();
  });

  it('refuses a name no substitution would answer', () => {
    expect(variableAt('{{ not a name }}', 4)).toBeNull();
  });
});

describe('a name the project says is a credential', () => {
  it('is not printed, however ordinary the word looks', () => {
    const told = { ...env, variables: { SEED: 'abc123' }, secret: ['SEED'] };
    const said = hoverOver('{{SEED}}', told);
    expect(said).not.toContain('abc123');
    expect(said).toContain('••••••');
  });

  it('still prints the values it was not told about', () => {
    const told = { ...env, variables: { HOST: 'api.test' }, secret: ['SEED'] };
    expect(hoverOver('{{HOST}}', told)).toContain('api.test');
  });
});
