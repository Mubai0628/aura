import type { CoreBusyActivityV1, CoreBusyV1 } from '@aura/contracts/generated';
import type { CoreStatusPresentation } from './app-shell-types';

/** What Core is busy with, in the status at the window's foot. */
const DOING: Record<CoreBusyActivityV1, string> = {
  deleting_session: '正在删除会话',
  removing_workspace: '正在删除工作区的记录',
  finishing_deletion: '正在完成上次没删完的内容',
};

export function busyPresentation(busy: CoreBusyV1): CoreStatusPresentation {
  return {
    label: `Core 正忙：${DOING[busy.activity]}`,
    tone: 'warning',
    detail: '这项工作要占用 Aura 的数据库一会儿；其他操作会排队，完成后自动继续，不必重试。',
  };
}
