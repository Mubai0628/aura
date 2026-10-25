import { useEffect, useState } from 'react';
import type { PackageSummaryV1 } from '@aura/contracts/generated';

/**
 * Installed package names, read once for display. Labels never decide anything: a
 * failed or missing read leaves the id-based label in place.
 */
export function usePackageSummaries(enabled: boolean): PackageSummaryV1[] | undefined {
  const [packages, setPackages] = useState<PackageSummaryV1[]>();
  useEffect(() => {
    if (!enabled) return;
    let live = true;
    try {
      void window.aura.v1.packages
        .invoke('packages.list', {})
        .then((result) => {
          if (live && result.ok) setPackages(result.value.packages);
        })
        .catch(() => undefined);
    } catch {
      // No package bridge: keep the id-based label.
    }
    return () => {
      live = false;
    };
  }, [enabled]);
  return packages;
}

/** The package's own name when known, otherwise a short form of its id. */
export function packageDisplayName(
  packages: readonly PackageSummaryV1[] | undefined,
  packageId: string,
): string {
  return (
    packages?.find((summary) => summary.package_id === packageId)?.display_name ??
    `ID ${packageId.slice(4, 16)}…`
  );
}
