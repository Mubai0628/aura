import type { NotificationInput } from './notification-center-store';
import type { useSecurityControl } from './security-control-store';
import { HostPreparationPanel } from './HostPreparationPanel';
import { PermissionDefaultsCard } from './security/PermissionDefaultsCard';
import './security/security-ui.css';

export function SecuritySettings({
  security,
  onNotify,
}: {
  security: ReturnType<typeof useSecurityControl>;
  onNotify: (notification: NotificationInput) => void;
}) {
  return (
    <div className="security-settings">
      <div className="settings-content-heading">
        <div>
          <h2>权限与沙箱</h2>
        </div>
      </div>
      <div className="security-settings-grid">
        <PermissionDefaultsCard onNotify={onNotify} />
        <HostPreparationPanel security={security} />
      </div>
    </div>
  );
}
