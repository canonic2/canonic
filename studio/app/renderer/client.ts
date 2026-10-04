import type { Action, Requests, Response } from '../contracts.ts';
export async function invoke<A extends Action>(
  action: A,
  payload?: Requests[A],
): Promise<Response<A>> {
  const result = await window.studio.invoke(action, payload);
  if (!result.ok) throw new Error(result.error);
  return result.value;
}
