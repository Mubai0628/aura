import type { SecurityProjectionChange, TerminalViewV1 } from '@aura/contracts/generated';
import { parseSecurityProjection } from '../shared/p04-validation.js';
import { parseAgentResult } from '../shared/agent-validation.js';
import {
  auxiliaryNotificationKey,
  isAuxiliaryRevision,
} from '../shared/auxiliary-projection-validation.js';
import { exactRecord, isRecord } from '../shared/projection-validation/common.js';
import type { ProjectionCompatibility } from './projection-compatibility.js';

export function readSecurityNotification(
  value: unknown,
  runtime: string,
  compatibility: ProjectionCompatibility,
  supported: boolean,
): SecurityProjectionChange | undefined {
  return compatibility.readAuxiliary(
    auxiliaryNotificationKey('security.projection.change', value),
    runtime,
    () => {
      if (!supported || !isRecord(value) || !isAuxiliaryRevision(value.revision)) return undefined;
      return parseSecurityProjection(value);
    },
  );
}

export function readTerminalStateNotification(
  value: unknown,
  runtime: string,
  compatibility: ProjectionCompatibility,
): TerminalViewV1 | undefined {
  return compatibility.readAuxiliary(
    auxiliaryNotificationKey('terminal.state.v1', value),
    runtime,
    () => {
      if (
        !exactRecord(value, ['runtime_instance_id', 'view']) ||
        !isRecord(value.view) ||
        !isAuxiliaryRevision(value.view.revision)
      )
        return undefined;
      return parseAgentResult('terminal.user.list', { terminals: [value.view], next_cursor: null })
        .terminals[0];
    },
  );
}
