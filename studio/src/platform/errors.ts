export function asError(value: unknown): Error {
  return value instanceof Error ? value : new Error(String(value));
}

export function errorMessage(value: unknown): string {
  return asError(value).message;
}

export function errorCode(value: unknown): string | undefined {
  return value !== null &&
    typeof value === 'object' &&
    'code' in value &&
    typeof value.code === 'string'
    ? value.code
    : undefined;
}
