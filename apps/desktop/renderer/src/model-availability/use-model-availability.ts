import type {
  ModelAvailabilityTestStartAccepted,
  ModelAvailabilityTestView,
  ModelEndpointConfig,
  ModelProfileConfig,
} from '@aura/contracts/generated';
import { useEffect, useRef, useState } from 'react';

import type { NotificationInput } from '../notification-center-store';
import { userMessage } from '../agent-session/user-message';

/** Presentation belongs to this mounted card, never to the persisted last test. */
export function useModelAvailability(
  model: ModelProfileConfig,
  endpoint: ModelEndpointConfig,
  notify: (notification: NotificationInput) => void,
) {
  const [accepted, setAccepted] = useState<ModelAvailabilityTestStartAccepted>();
  const [pending, setPending] = useState(false);
  const [finished, setFinished] = useState(false);
  const mounted = useRef(true);
  const inFlight = useRef(false);
  const reported = useRef<string | undefined>(undefined);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const start = async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    setPending(true);
    setAccepted(undefined);
    setFinished(false);
    try {
      const result = await window.aura.v1.testModelAvailability({
        expected_endpoint_revision: endpoint.revision,
        expected_model_profile_revision: model.revision,
        model_profile_id: model.id,
      });
      if (!mounted.current) return;
      if (!result.ok) {
        notify({
          dedupeKey: `model-availability-start:${model.id}`,
          title: '模型测试未能开始',
          message: userMessage(result.error, '模型测试没有开始，请刷新模型设置后重试。'),
          tone: 'danger',
        });
        inFlight.current = false;
        return;
      }
      setAccepted(result.value);
    } catch {
      if (mounted.current) {
        notify({
          dedupeKey: `model-availability-unknown:${model.id}`,
          title: '模型测试状态未确认',
          message: '连接中断，请等待状态同步后再试。',
          tone: 'warning',
        });
      }
      inFlight.current = false;
    } finally {
      if (mounted.current) setPending(false);
    }
  };

  const complete = (test: ModelAvailabilityTestView) => {
    if (test.id !== accepted?.model_availability_test_id || reported.current === test.id) return;
    reported.current = test.id;
    inFlight.current = false;
    setFinished(true);
    const cancelled = test.state === 'cancelled';
    const available = test.result?.outcome === 'available';
    notify({
      dedupeKey: `model-availability-terminal:${test.id}`,
      title: cancelled ? '模型测试已取消' : available ? '模型可用' : '模型不可用',
      message: cancelled
        ? `${model.display_name} 的可用性测试已取消。`
        : available
          ? `${model.display_name} 已通过模型生成验证。`
          : `${model.display_name} 未通过验证，请查看分项结果。`,
      tone: cancelled ? 'info' : available ? 'success' : 'warning',
    });
  };

  return {
    accepted,
    complete,
    pending,
    start,
    testing: pending || (accepted !== undefined && !finished),
  };
}
