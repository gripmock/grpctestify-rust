import type { GctfDiagnostic } from './types';
import { rewriteOf } from './text-edit';

export interface ProblemFix {
  id: 'expect-response' | 'name-address' | 'apply-rewrite';
  label: string;
  title: string;
}

export interface FixContext {
  hasResponse: boolean;
  failed: boolean;
  http?: boolean;
  addressFromHeader?: string | null;
  editable?: boolean;
}

export function fixFor(problem: GctfDiagnostic, ctx: FixContext): ProblemFix | null {
  const rewrite = rewriteOf(problem);
  if (rewrite !== null && ctx.editable) {
    const isKey = typeof problem.data?.suggested_key === 'string';
    const isMetaList = problem.code === 'META_LIST_EXPECTED';
    const isOptimizer = problem.message.startsWith('Optimizer hint');
    return {
      id: 'apply-rewrite',
      label: isKey ? `use ${rewrite}` : isMetaList ? 'write it as a list' : 'rewrite it',
      title: isKey
        ? `Write ${rewrite} instead — the spelling this key has now`
        : isMetaList
          ? `Replace this line with ${rewrite} — the list form the runner reads`
          : isOptimizer
            ? `Replace this line with ${rewrite} — the rewrite \`grpctestify fmt -O\` would make`
            : `Replace this line with ${rewrite} — the form the runner reads`,
    };
  }

  if (problem.message.startsWith('ADDRESS section missing')) {
    const address = ctx.addressFromHeader?.trim();
    if (!address) return null;
    return {
      id: 'name-address',
      label: `name ${address} here`,
      title: `Write an ADDRESS section with ${address} — the address this workbench is aimed at. A run reads the file, never the header.`,
    };
  }

  if (!/^(Document \d+: )?(At least one verification section|Nothing verifies the answer)/.test(problem.message)) return null;
  if (!ctx.hasResponse) return null;
  if (ctx.http) {
    return {
      id: 'expect-response',
      label: 'expect the answer',
      title: 'Write @status() into ASSERTS and this answer as RESPONSE — an HTTP failure is an answer that arrived, checked by its status',
    };
  }
  return {
    id: 'expect-response',
    label: ctx.failed ? 'expect this failure' : 'expect the answer',
    title: ctx.failed
      ? 'Write an ERROR section from the failure this tab got back'
      : 'Write a RESPONSE section from the answer this tab got back',
  };
}
