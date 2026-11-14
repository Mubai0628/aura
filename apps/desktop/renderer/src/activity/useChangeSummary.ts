import { useEffect, useSyncExternalStore } from 'react';
import { reviewTotals } from '../review/review-model';
import { checkReview, reviewKey, reviewSnapshot, subscribeReviews } from '../review/review-store';

/** Why the uncommitted changes cannot be summed up. */
export type ChangeSummaryGap = 'no-git' | 'too-old' | 'invalid' | 'not-repository' | 'unreadable';

export type ChangeSummary =
  | { state: 'reading' }
  | { state: 'unavailable'; reason: ChangeSummaryGap }
  | {
      state: 'read';
      files: number;
      added: number;
      deleted: number;
      uncounted: number;
      partial: boolean;
    };

/**
 * The workspace's uncommitted changes as
 * the review page reads them — the same store, so both say the same — read again whenever
 * `refreshKey` changes (a turn ended, files may have changed).
 */
export function useChangeSummary(
  workspace: string | undefined,
  session: string | undefined,
  refreshKey: string,
): ChangeSummary | undefined {
  const key = workspace ? reviewKey(workspace, null, false) : '';
  const snapshot = useSyncExternalStore(subscribeReviews, () =>
    key ? reviewSnapshot(key) : undefined,
  );
  useEffect(() => {
    if (!workspace || !session) return;
    checkReview({ workspace, session, base: null, ignoreWhitespace: false });
  }, [workspace, session, refreshKey]);
  if (!workspace) return undefined;
  if (!snapshot) return { state: 'reading' };
  if (snapshot.runtime && snapshot.runtime.state !== 'available') {
    const state: string = snapshot.runtime.state;
    const reason = state === 'too_old' ? 'too-old' : state === 'invalid' ? 'invalid' : 'no-git';
    return { state: 'unavailable', reason };
  }
  if (snapshot.notRepository) return { state: 'unavailable', reason: 'not-repository' };
  if (!snapshot.status)
    return snapshot.error ? { state: 'unavailable', reason: 'unreadable' } : { state: 'reading' };
  return { state: 'read', ...reviewTotals(snapshot.files), partial: snapshot.partial };
}
