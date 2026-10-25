import type { ExecutionApprovalView } from '@aura/contracts/generated';

interface BoundFile {
  path: string;
  content: string;
  previous: string | null;
}

const actionOf = (view: ExecutionApprovalView): Record<string, unknown> => {
  const action = view.operation.content.action;
  return action && typeof action === 'object' && !Array.isArray(action)
    ? (action as Record<string, unknown>)
    : {};
};

/** The exact files Core bound into the Operation; nothing here is re-rendered. */
function boundFiles(view: ExecutionApprovalView): BoundFile[] {
  const files = actionOf(view).files;
  if (!Array.isArray(files)) return [];
  return files.flatMap((file: unknown) => {
    if (!file || typeof file !== 'object') return [];
    const { path, content, previous } = file as Record<string, unknown>;
    return typeof path === 'string' && typeof content === 'string'
      ? [{ path, content, previous: typeof previous === 'string' ? previous : null }]
      : [];
  });
}

function BoundFileList({ files }: { files: BoundFile[] }) {
  return (
    <ul>
      {files.map((file) => (
        <li key={file.path}>
          <details>
            <summary>
              {file.previous === null ? '新建' : '替换'} {file.path}
            </summary>
            <pre>{file.content}</pre>
            {file.previous === null ? null : (
              <details>
                <summary>被替换的原内容</summary>
                <pre>{file.previous}</pre>
              </details>
            )}
          </details>
        </li>
      ))}
    </ul>
  );
}

/** `/init`'s one answer: exactly these files with exactly this text. */
export function WorkspaceInitApprovalFacts({ view }: { view: ExecutionApprovalView }) {
  const files = boundFiles(view);
  return (
    <div className="agent-approval-init" aria-label="初始化写入内容">
      <p>
        初始化工作区：一次写入以下 {files.length}{' '}
        个文件，全部写入或都不写。写入前文件若被改动，不会覆盖。
      </p>
      <BoundFileList files={files} />
    </div>
  );
}

/** A workspace command change: the files a save writes, or the folder a removal takes. */
export function CommandApprovalFacts({ view }: { view: ExecutionApprovalView }) {
  const action = actionOf(view);
  const input = (action.input ?? {}) as Record<string, unknown>;
  const name = typeof input.name === 'string' ? input.name : '';
  if (view.operation.content.action_kind === 'commands.delete') {
    const template = typeof action.template === 'string' ? action.template : null;
    return (
      <div className="agent-approval-init" aria-label="命令删除内容">
        <p>
          删除工作区命令 /{name}：移除 {typeof action.folder === 'string' ? action.folder : ''}{' '}
          整个目录。
        </p>
        {template === null ? null : (
          <details>
            <summary>被删除的模板</summary>
            <pre>{template}</pre>
          </details>
        )}
      </div>
    );
  }
  const files = boundFiles(view);
  return (
    <div className="agent-approval-init" aria-label="命令写入内容">
      <p>
        {action.created === true ? '新建' : '替换'}工作区命令 /{name}：写入以下 {files.length}{' '}
        个文件，全部写入或都不写。命令只是你发送前可以查看、修改的消息文字，不授予任何权限。
      </p>
      <BoundFileList files={files} />
    </div>
  );
}
