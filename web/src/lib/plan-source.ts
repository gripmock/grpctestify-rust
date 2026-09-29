export type AddressOrigin = 'file' | 'environment' | 'workbench';

export function addressOrigin(input: {
  own: boolean;
  fromChain: boolean;
  source: string;
}): AddressOrigin {
  if (input.own || input.fromChain) return 'file';
  return input.source === 'environment' ? 'environment' : 'workbench';
}

export function originClass(origin: AddressOrigin): string {
  return `plan-from is-${origin}`;
}
