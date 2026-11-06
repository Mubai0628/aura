import type { PluginView } from '@aura/contracts/generated';
import { ToggleSwitch } from '../ui/ToggleSwitch';
import { serviceStatusLabel } from './plugin-labels';

/** A plugin's background service on its card: its switch and what it is doing now. */
export function PluginService({
  plugin,
  busy,
  onSwitch,
}: {
  plugin: PluginView;
  busy: boolean;
  onSwitch: (on: boolean) => void;
}) {
  const service = plugin.service;
  if (!service) return null;
  const shown = serviceStatusLabel(service.status);
  return (
    <section className="plugin-service" aria-label="后台服务">
      <div className="plugin-service-heading">
        <strong>后台服务</strong>
        <span className="status-pill" data-tone={shown.tone}>
          {shown.label}
        </span>
        <ToggleSwitch
          checked={service.on}
          disabled={busy}
          label={`${plugin.name} 的后台服务`}
          onCheckedChange={onSwitch}
        />
      </div>
      {shown.detail ? (
        <p className="plugin-service-detail" role={shown.tone === 'danger' ? 'alert' : undefined}>
          {shown.detail}
        </p>
      ) : null}
    </section>
  );
}
