export function errorText(error: string): string {
  const match = /^gRPC error code=\d+ message=(.*)$/s.exec(error.trim());
  return match ? match[1].trim() : error;
}
