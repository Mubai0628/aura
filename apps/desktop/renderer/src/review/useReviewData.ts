import { useCallback, useEffect, useSyncExternalStore } from 'react';
import { defaultBranchOf, type Comparison } from './review-model';
import { checkReview, reviewKey, reviewSnapshot, subscribeReviews } from './review-store';

/**
 * What the review page compares. It comes from the review store, which
 * keeps it while the app runs — the page shows what was read last at once and Git is checked
 * quietly behind it whenever the page opens or is refreshed.
 */
export function useReviewData(
  workspace: string,
  session: string,
  comparison: Comparison,
  ignoreWhitespace: boolean,
) {
  const base = comparison.kind === 'branch' ? comparison.branch : null;
  const key = reviewKey(workspace, base, ignoreWhitespace);
  const snapshot = useSyncExternalStore(subscribeReviews, () => reviewSnapshot(key));
  const refresh = useCallback(
    () => checkReview({ workspace, session, base, ignoreWhitespace }),
    [workspace, session, base, ignoreWhitespace],
  );
  useEffect(refresh, [refresh]);

  return {
    ...snapshot,
    files: snapshot?.files ?? [],
    partial: snapshot?.partial ?? false,
    /** Nothing was read of this comparison yet. */
    loading: !snapshot?.runtime && !snapshot?.error,
    /** Git is being read, maybe behind what is shown. */
    checking: snapshot?.checking ?? true,
    defaultBranch: snapshot?.status
      ? defaultBranchOf(snapshot.status.branches, snapshot.status.upstream)
      : undefined,
    refresh,
  };
}
