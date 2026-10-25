import { useEffect, useState } from 'react';
import { toolTabs } from '../right-workspace/tool-tabs';
import { filesNamed } from './file-locate';
import type { FileScope } from './file-tree-store';

/**
 * A bare name the model wrote that is not at the workspace root, with several
 * files of that name: they are listed, and one click opens it.
 */
export function SameNameFiles({
  scope,
  name,
  line,
}: {
  scope: FileScope;
  name: string;
  line?: number | undefined;
}) {
  const [found, setFound] = useState<string[]>([]);
  useEffect(() => {
    let current = true;
    void filesNamed(scope, name).then((paths) => current && setFound(paths));
    return () => {
      current = false;
    };
  }, [scope, name]);
  if (found.length < 2) return null;
  return (
    <ul aria-label={`同名文件：${name}`} className="file-view-same-names">
      {found.map((path) => (
        <li key={path}>
          <button onClick={() => toolTabs.revealFile(scope.session, path, line)} type="button">
            {path}
          </button>
        </li>
      ))}
    </ul>
  );
}
