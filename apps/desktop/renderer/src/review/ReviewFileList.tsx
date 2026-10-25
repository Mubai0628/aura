import { FileCounts } from './DiffCard';
import { byFolder, KIND_LABELS, nameOf, type FileKind, type ReviewFile } from './review-model';

/** The list shows this many files; the rest are on the page below it. */
const LISTED = 1000;

function Row({
  file,
  label,
  onPick,
}: {
  file: ReviewFile;
  label: string;
  onPick: (file: ReviewFile) => void;
}) {
  return (
    <li>
      <button
        className="review-list-row"
        onClick={() => onPick(file)}
        title={file.path}
        type="button"
      >
        <span className="review-list-name">{label}</span>
        <FileCounts file={file} />
      </button>
    </li>
  );
}

/**
 * The changed files, by path or by folder, with tests and what builds write
 * apart when asked. Picking one shows its changes.
 */
export function ReviewFileList({
  groups,
  folders,
  onPick,
}: {
  groups: { kind?: FileKind; files: ReviewFile[] }[];
  folders: boolean;
  onPick: (file: ReviewFile) => void;
}) {
  const listed: { kind?: FileKind; files: ReviewFile[] }[] = [];
  let room = LISTED;
  for (const group of groups) {
    const files = group.files.slice(0, room);
    room -= files.length;
    listed.push({ ...group, files });
  }
  const total = groups.reduce((sum, group) => sum + group.files.length, 0);
  return (
    <nav aria-label="改动的文件" className="review-file-list">
      {listed.map((group) => {
        const files = group.files;
        if (!files.length) return null;
        return (
          <section key={group.kind ?? 'all'}>
            {group.kind ? (
              <h4 className="review-list-group">
                {KIND_LABELS[group.kind]}
                <span>{group.files.length}</span>
              </h4>
            ) : null}
            {folders ? (
              byFolder(files).map((folder) => (
                <div className="review-list-folder" key={folder.folder}>
                  <p title={folder.folder}>{folder.folder || '根目录'}</p>
                  <ul>
                    {folder.files.map((file) => (
                      <Row file={file} key={file.path} label={nameOf(file.path)} onPick={onPick} />
                    ))}
                  </ul>
                </div>
              ))
            ) : (
              <ul>
                {files.map((file) => (
                  <Row file={file} key={file.path} label={file.path} onPick={onPick} />
                ))}
              </ul>
            )}
          </section>
        );
      })}
      {total > LISTED ? (
        <p className="review-list-more">
          还有 {total - LISTED} 个文件未列出，可在下方或用搜索找到。
        </p>
      ) : null}
    </nav>
  );
}
