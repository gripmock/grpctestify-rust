export function previewRequest(
  payload: Record<string, unknown>,
  where: { path: string; originalPath?: string | null; activeStep: number },
): Record<string, unknown> {
  return {
    ...payload,
    path: where.path,
    original_path: where.originalPath ?? undefined,
    document_index: where.activeStep,
  };
}
