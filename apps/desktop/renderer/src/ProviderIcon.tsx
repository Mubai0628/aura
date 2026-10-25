import { useState } from 'react';

interface ProviderIconProps {
  fallbackInitial: string;
  iconUrl: string | undefined;
}

/**
 * Renders only the Core-validated provider-icon protocol URL. A missing,
 * evicted, or unreadable SystemCache artifact falls back without changing the
 * fixed icon box geometry.
 */
export function ProviderIcon({ fallbackInitial, iconUrl }: ProviderIconProps) {
  const [failedUrl, setFailedUrl] = useState<string>();
  if (!iconUrl || failedUrl === iconUrl) {
    return <span className="provider-initials">{fallbackInitial.trim().toUpperCase() || '?'}</span>;
  }
  return <img alt="" onError={() => setFailedUrl(iconUrl)} src={iconUrl} />;
}
