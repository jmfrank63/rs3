// Static sync-only source gate (documented limitation): QuickJS async
// functions bypass any global shadowing (research row 12 — shadowing the
// `Promise` global does NOT stop `async`/`await`), and pending jobs never
// run in the sync sandbox, so async guest code would silently hang. Guests
// are rejected by SOURCE INSPECTION before eval — conservative by design
// (a comment containing "async" is rejected too); that is the trade.
export const ASYNC_SOURCE_PATTERN = /\basync\b|\bawait\b|\bPromise\b/;

export function findAsyncUsage(code: string): string | null {
  const match = ASYNC_SOURCE_PATTERN.exec(code);
  return match === null ? null : match[0];
}
