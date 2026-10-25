import type {
  ModelAvailabilityTestStartAccepted,
  ModelAvailabilityTestView,
} from '@aura/contracts/generated';
import { useMemo } from 'react';

import { ModelAvailabilityStatus } from '../ModelOperationStatus';
import { useScopedModelProjection } from '../model-projection-store';
import type { NotificationInput } from '../notification-center-store';

export function AvailabilitySkeleton() {
  return (
    <section className="availability-skeleton" aria-busy="true" aria-label="模型可用性测试进度">
      <strong role="status">正在测试模型可用性…</strong>
      <span className="skeleton-bar skeleton-heading" aria-hidden="true" />
      <div className="skeleton-checks" aria-hidden="true">
        {Array.from({ length: 6 }, (_, index) => (
          <span className="skeleton-bar" key={index} />
        ))}
      </div>
    </section>
  );
}

/** Only subscribe to the id returned by this page's explicit click. */
export function RequestedModelAvailability({
  accepted,
  onNotify,
  onTerminal,
}: {
  accepted: ModelAvailabilityTestStartAccepted;
  onNotify: (notification: NotificationInput) => void;
  onTerminal: (test: ModelAvailabilityTestView) => void;
}) {
  const route = useMemo(
    () =>
      ({
        projection_kind: 'model_availability_test',
        projection_scope: {
          type: 'model_availability_test',
          model_availability_test_id: accepted.model_availability_test_id,
        },
      }) as const,
    [accepted.model_availability_test_id],
  );
  const test = useScopedModelProjection(route)?.payload.test;
  if (test === undefined) return <AvailabilitySkeleton />;
  return <ModelAvailabilityStatus test={test} onNotify={onNotify} onTerminal={onTerminal} />;
}
