import { useState } from 'react';

import { useNearView } from './ui/useNearView';

interface ProviderIconProps {
  fallbackInitial: string;
  iconUrl: string | undefined;
}

/**
 * Renders only the Core-validated provider-icon protocol URL. A missing,
 * evicted, or unreadable SystemCache artifact falls back without changing the
 * fixed icon box geometry.
 *
 * A catalog lists hundreds of providers and every icon is a read from Core, so an icon is asked
 * for only once it comes near the view; until then its box stays empty.
 */
export function ProviderIcon({ fallbackInitial, iconUrl }: ProviderIconProps) {
  const [failedUrl, setFailedUrl] = useState<string>();
  const [waiting, near] = useNearView<HTMLSpanElement>();
  if (!iconUrl || failedUrl === iconUrl) {
    return <span className="provider-initials">{fallbackInitial.trim().toUpperCase() || '?'}</span>;
  }
  if (!near) return <span aria-hidden="true" ref={waiting} />;
  return (
    <img
      alt=""
      decoding="async"
      loading="lazy"
      onError={() => setFailedUrl(iconUrl)}
      src={iconUrl}
    />
  );
}
