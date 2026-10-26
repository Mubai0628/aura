import type { CoreBusyActivityV1, CoreBusyV1 } from '@aura/contracts/generated';
import type { CoreStatusPresentation } from './app-shell-types';

/** What Core is deleting in the background, in the status at the window's foot. */
const DOING: Record<CoreBusyActivityV1, string> = {
  deleting_session: '正在后台删除会话记录',
  removing_workspace: '正在后台删除工作区记录',
  finishing_deletion: '正在后台删除上次没删完的记录',
};

export function busyPresentation(busy: CoreBusyV1): CoreStatusPresentation {
  return { label: DOING[busy.activity], tone: 'muted' };
}
