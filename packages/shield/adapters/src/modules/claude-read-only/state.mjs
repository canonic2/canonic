// Denial wording and bounded per-factory correlation state for SDK callbacks.
// Neither map grants permission; the core decides every request.
import { FAILURE_CODES } from '../enforcement-response/index.mjs';

const text = (value, max = 4096) => typeof value === 'string' && value.length > 0
  && Buffer.byteLength(value) <= max && !value.includes('\0');

/** Retained allowed reads awaiting their completion callback. */
export const PENDING_LIMIT = 512;
/** Retained approval references awaiting an operator decision or retry. */
export const APPROVAL_LIMIT = 128;
/** A pending read request lasts 10 minutes and its grant at most an hour after issue. */
export const APPROVAL_RETENTION_MS = (10 + 60) * 60 * 1000;

/** The host-visible reason for a denied SDK read. */
export function denialReason(code, request, detail, project) {
  if (code === 'policy-deny' && detail) return `Shield: denied. ${detail}`;
  if (request) return `Shield: approval required for read ${request}. Ask the user to review it with shield request show ${request} and approve or deny it. Retry only after approval.`;
  if (code === 'approval-unavailable') return 'Shield blocked this read (approval-unavailable). Submit a fresh action request for operator review.';
  if (['approval-stale', 'approval-expired', 'approval-consumed', 'read-changed'].includes(code)) {
    return `Shield blocked this read (${code}): the request changed or its approval is no longer usable. Retry to submit a fresh request for operator review.`;
  }
  if (code === 'approval-revoked') return 'Shield blocked this read (approval-revoked): the user revoked or denied this approval. Do not retry automatically.';
  if (code === 'policy-deny') return 'Shield: denied by active policy. Stop this read; use Shield MCP to explain the matching rule.';
  if (code === 'compiled-cache-unavailable' && text(project) && !/\p{Cc}/u.test(project)) {
    return `Shield: compiled policy unavailable. Ask the user to run \`shield cache prepare --project '${project.replaceAll("'", "'\\''")}'\`; retry after it succeeds.`;
  }
  if (code === 'runtime-busy') return 'Shield state is briefly locked; retry the request.';
  if (code === 'invalid-context') return 'Shield: the session\'s working directory is not an existing folder inside this Shield project. Work from the project folder, or ask the user to start the session there.';
  if (code.startsWith('invalid-')) return `Shield: invalid request (${code}). Correct the tool input before retrying.`;
  if (['unsupported-operation', 'unverified-read'].includes(code)) return `Shield: unsupported tool or read (${code}). Explain the limitation; do not work around the block.`;
  return `Shield could not verify this request (${code}). Ask the user to run shield doctor before retrying.`;
}

/**
 * Allowed reads by host tool identity. An entry outlives its completion, so a
 * redelivered completion keeps its correlation and a reused host ID stays
 * ambiguous; the oldest entry gives way when the map is full.
 */
export class PendingReads {
  #entries = new Map();
  get size() { return this.#entries.size; }
  /** Record an allowed read; a repeated identity becomes ambiguous and correlates nothing. */
  allow(identity, entry) {
    if (this.#entries.has(identity)) {
      this.#entries.set(identity, null);
      return;
    }
    if (this.#entries.size >= PENDING_LIMIT) this.#entries.delete(this.#entries.keys().next().value);
    this.#entries.set(identity, entry);
  }
  /** The read a completion reports; null when its host identity was reused. */
  complete(identity) {
    return this.#entries.get(identity);
  }
}

/** Approval references by exact request, dropped after their usable lifetime. */
export class ApprovalReferences {
  #entries = new Map();
  #now;
  constructor(now = Date.now) { this.#now = now; }
  get size() { return this.#entries.size; }
  #prune() {
    const oldest = this.#now() - APPROVAL_RETENTION_MS;
    for (const [key, entry] of this.#entries) {
      if (entry.at > oldest) break;
      this.#entries.delete(key);
    }
  }
  get(key) {
    this.#prune();
    return this.#entries.get(key)?.reference;
  }
  /** Retain a pending reference, keeping its first time; the oldest goes when full. */
  pending(key, reference) {
    this.#prune();
    const prior = this.#entries.get(key);
    if (prior?.reference === reference) return;
    this.#entries.delete(key);
    if (this.#entries.size >= APPROVAL_LIMIT) this.#entries.delete(this.#entries.keys().next().value);
    this.#entries.set(key, { reference, at: this.#now() });
  }
  delete(key) { this.#entries.delete(key); }
  /**
   * Track a request's reference from its receipt. A pending request keeps it;
   * a receipt the core could not decide, such as a briefly locked runtime,
   * keeps it too, so a retry still consumes the same approval. A final
   * approval outcome or a policy decision without approval drops it.
   */
  record(key, result) {
    if (result.approval.status === 'pending') this.pending(key, result.approval.request_id);
    else if (!FAILURE_CODES.includes(result.code)) this.delete(key);
  }
}
