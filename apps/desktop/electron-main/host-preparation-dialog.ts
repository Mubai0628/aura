import type { HostPreparationOperationView } from '@aura/contracts/generated';
import {
  HOST_ACTION_SUMMARY,
  HOST_ACTION_TITLE,
  hostPlanLines,
  hostPlanValidUntil,
} from '../shared/host-preparation-text.js';

type PreparedPlan = Pick<
  HostPreparationOperationView,
  | 'action'
  | 'add_all_application_packages_ace'
  | 'add_all_restricted_application_packages_ace'
  | 'expires_at'
  | 'update_null_device_security'
>;

/**
 * The native confirmation for a prepared plan: the action, what it changes in Windows, and until
 * when the plan holds, in words.
 */
export function hostPreparationDialog(
  operation: PreparedPlan,
  now = new Date(),
): { title: string; message: string; detail: string } {
  return {
    title: 'Aura 兼容环境',
    message: HOST_ACTION_TITLE[operation.action],
    detail: [
      HOST_ACTION_SUMMARY[operation.action],
      hostPlanValidUntil(operation.expires_at, now),
      ...hostPlanLines(operation).map((line) => `• ${line}`),
    ].join('\n'),
  };
}
