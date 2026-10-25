import type { GitBranchV1 } from '@aura/contracts/generated';
import { useRef } from 'react';
import { AppIcon } from '../app-icons';
import { ToolIcon } from '../tool-icon';
import { useCompactRow } from '../ui/useCompactRow';
import { useDismissablePopover } from '../useDismissablePopover';
import { branchName, comparisonLabel, type Comparison } from './review-model';
import { setReviewOptions, type ReviewOptions } from './review-options';

const TOGGLES: [keyof ReviewOptions, string][] = [
  ['fileList', '显示文件列表'],
  ['byFolder', '按文件夹分组'],
  ['separate', '测试与生成文件分开'],
  ['split', '左右对照'],
  ['wrap', '自动换行'],
  ['words', '高亮改动的词'],
  ['ignoreWhitespace', '隐藏空白改动'],
];

/**
 * The review page's toolbar after the owner's reference — what to compare
 * with, search, the view options, and filling the main area or closing the page.
 */
export function ReviewToolbar({
  comparison,
  defaultBranch,
  branches,
  onCompare,
  options,
  searching,
  onSearch,
  onCollapseAll,
  onExpandAll,
  onRefresh,
  expanded,
  onExpand,
  onClose,
}: {
  comparison: Comparison;
  defaultBranch?: string | undefined;
  branches: GitBranchV1[];
  onCompare: (comparison: Comparison) => void;
  options: ReviewOptions;
  searching: boolean;
  onSearch: () => void;
  onCollapseAll: () => void;
  onExpandAll: () => void;
  onRefresh: () => void;
  expanded: boolean;
  onExpand: () => void;
  onClose: () => void;
}) {
  const compareRef = useRef<HTMLDivElement>(null);
  const [comparing, setComparing] = useDismissablePopover(compareRef);
  // Too narrow for the comparison's words, its button shows its icon.
  const row = useCompactRow<HTMLDivElement>();
  const moreRef = useRef<HTMLDivElement>(null);
  const [more, setMore] = useDismissablePopover(moreRef);
  const compare = (next: Comparison) => {
    setComparing(false);
    onCompare(next);
  };
  const others = branches
    .map((branch) => branchName(branch.name))
    .filter((name) => name !== defaultBranch && !name.endsWith('/HEAD'));
  const act = (run: () => void) => () => {
    setMore(false);
    run();
  };
  return (
    <div aria-label="审查工具" className="review-toolbar" ref={row} role="toolbar">
      <button
        aria-label={options.fileList ? '隐藏文件列表' : '显示文件列表'}
        aria-pressed={options.fileList}
        className="icon-button"
        onClick={() => setReviewOptions({ fileList: !options.fileList })}
        title={options.fileList ? '隐藏文件列表' : '显示文件列表'}
        type="button"
      >
        <AppIcon name="sidebar-left" size={15} />
      </button>
      <div className="review-compare" ref={compareRef}>
        <button
          aria-expanded={comparing}
          aria-haspopup="menu"
          className="review-compare-button"
          data-adaptive
          onClick={() => setComparing((value) => !value)}
          title={`比较范围：${comparisonLabel(comparison, defaultBranch)}`}
          type="button"
        >
          <ToolIcon name="file-diff" size={13} />
          <span data-adaptive-label>{comparisonLabel(comparison, defaultBranch)}</span>
          <AppIcon className="adaptive-chevron" name="chevron-down" size={13} />
        </button>
        {comparing ? (
          <div aria-label="比较范围" className="review-menu" role="menu">
            <button
              aria-checked={comparison.kind === 'branch' && comparison.branch === defaultBranch}
              disabled={!defaultBranch}
              onClick={() => defaultBranch && compare({ kind: 'branch', branch: defaultBranch })}
              role="menuitemradio"
              type="button"
            >
              {defaultBranch
                ? `全部改动（与 ${defaultBranch} 比较）`
                : '全部改动（没有找到默认分支）'}
            </button>
            <button
              aria-checked={comparison.kind === 'uncommitted'}
              onClick={() => compare({ kind: 'uncommitted' })}
              role="menuitemradio"
              type="button"
            >
              未提交的改动
            </button>
            <p className="review-menu-label">与其他分支比较</p>
            {others.length ? (
              <div className="review-menu-branches">
                {others.map((name) => (
                  <button
                    aria-checked={comparison.kind === 'branch' && comparison.branch === name}
                    key={name}
                    onClick={() => compare({ kind: 'branch', branch: name })}
                    role="menuitemradio"
                    title={name}
                    type="button"
                  >
                    {name}
                  </button>
                ))}
              </div>
            ) : (
              <p className="review-menu-note">没有其他分支</p>
            )}
          </div>
        ) : null}
      </div>
      <span className="review-toolbar-space" />
      <button
        aria-label="搜索改动"
        aria-pressed={searching}
        className="icon-button"
        onClick={onSearch}
        title="搜索改动"
        type="button"
      >
        <AppIcon name="search" size={15} />
      </button>
      <div className="review-more" ref={moreRef}>
        <button
          aria-expanded={more}
          aria-haspopup="menu"
          aria-label="更多审查选项"
          className="icon-button"
          onClick={() => setMore((value) => !value)}
          title="更多审查选项"
          type="button"
        >
          <AppIcon name="more" size={15} />
        </button>
        {more ? (
          <div aria-label="审查选项" className="review-menu is-end" role="menu">
            {TOGGLES.map(([name, label]) => (
              <button
                aria-checked={options[name]}
                key={name}
                onClick={() => setReviewOptions({ [name]: !options[name] })}
                role="menuitemcheckbox"
                type="button"
              >
                <AppIcon name="check" size={13} />
                {label}
              </button>
            ))}
            <hr />
            <button onClick={act(onCollapseAll)} role="menuitem" type="button">
              全部收起
            </button>
            <button onClick={act(onExpandAll)} role="menuitem" type="button">
              全部展开
            </button>
            <button onClick={act(onRefresh)} role="menuitem" type="button">
              刷新
            </button>
          </div>
        ) : null}
      </div>
      <button
        aria-label={expanded ? '还原审查页' : '放大审查页'}
        aria-pressed={expanded}
        className="icon-button"
        onClick={onExpand}
        title={expanded ? '还原审查页' : '放大审查页'}
        type="button"
      >
        <AppIcon name={expanded ? 'shrink' : 'expand'} size={15} />
      </button>
      <button
        aria-label="关闭审查页"
        className="icon-button"
        onClick={onClose}
        title="关闭审查页"
        type="button"
      >
        <AppIcon name="close" size={15} />
      </button>
    </div>
  );
}
