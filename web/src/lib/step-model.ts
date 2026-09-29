import type { CollectionParsed, DocumentSummary } from './types';

export function parsedForStep(head: CollectionParsed, step: DocumentSummary): CollectionParsed {
  return {
    ...head,
    endpoint: step.endpoint,
    address: step.address_source === 'section' ? step.address : '',
    headers: { ...step.headers },
    bodies: step.bodies.length > 0 ? [...step.bodies] : [''],
    asserts: [...step.asserts],
    extracts: { ...step.extracts },
    options: { ...step.options },
    tls: { ...step.tls },
    proto: { ...step.proto },
  };
}
