import { useMemo } from 'react';
import { ModelDiscoveryStatus } from '../ModelOperationStatus';
import { useScopedModelProjection } from '../model-projection-store';
import type { NotificationInput } from '../notification-center-store';

/** The latest model-discovery run of an endpoint, shown at the top of its editor. */
export function EndpointOperationHistory({
  endpointId,
  onNotify,
}: {
  endpointId: string;
  onNotify: (notification: NotificationInput) => void;
}) {
  const route = useMemo(
    () =>
      ({
        projection_kind: 'endpoint_detail',
        projection_scope: { endpoint_id: endpointId, type: 'endpoint' },
      }) as const,
    [endpointId],
  );
  const change = useScopedModelProjection(route);
  const discovery = change?.payload.latest_discovery_run;
  if (discovery === null || discovery === undefined) return null;
  return (
    <section className="endpoint-operation-history" aria-label="最近模型操作">
      <strong>最近操作</strong>
      <ModelDiscoveryStatus
        accepted={{
          model_discovery_run_id: discovery.id,
          operational_revision: discovery.revision,
          state: discovery.state,
        }}
        key={discovery.id}
        onNotify={onNotify}
      />
    </section>
  );
}
