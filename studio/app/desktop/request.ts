import type { WorkspaceMode } from '../contracts.ts';

export interface CommandPayload {
  projectId?: string;
  sessionId?: string;
  name?: string;
  base?: string;
  mode?: WorkspaceMode;
}
export function validatePayload(input: unknown): CommandPayload {
  if (!input || typeof input !== 'object' || Array.isArray(input))
    throw new Error('Invalid Studio request.');
  const payload = input as Record<string, unknown>;
  for (const key of ['projectId', 'sessionId', 'name', 'base', 'mode']) {
    const value = payload[key];
    if (value !== undefined && (typeof value !== 'string' || value.length > 1000))
      throw new Error(`Invalid ${key}.`);
  }
  return payload;
}
export function required(payload: CommandPayload, key: 'projectId' | 'sessionId' | 'name'): string {
  const value = payload[key];
  if (!value) throw new Error(`Missing ${key}.`);
  return value;
}
