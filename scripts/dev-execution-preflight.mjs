// Read-only execution-binding report for the ordinary `corepack pnpm dev` entry.
//
// This never selects a different Core, grants access, invokes Setup/Repair or
// changes an installation. It answers one question the whole point of which is
// that a developer should not have to guess: *is the restricted Shell prepared
// for this run, and if not, which exact fact is missing?*
//
// The comparison uses the same rule set the packaged Main process uses, so the two
// entries cannot disagree about whether a published object is the one this run is
// bound to. Both rule modules are imported from `apps/desktop/electron-main/`, which
// is where they live precisely so that they ship with the compiled Main; a dev-only
// copy under `scripts/` would be a second answer to the same question.
import { lstat } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import {
  CORE_BINDING_GAP_TEXT,
  describeCoreBinding,
  publishedCorePath,
  readExecutionInstallations,
  resolvePublishedCore,
} from '../apps/desktop/electron-main/core-binding.mjs';
import {
  expectedExecution,
  readDevelopmentBinding,
} from '../apps/desktop/electron-main/component-binding.mjs';

/** The execution publication inputs a BuildDirectory must carry. */
const REVIEW_FILES = [
  'aura-core.exe',
  'aura-command-runner.exe',
  'aura-execution-helper.exe',
  'aura-execution-maintainer.exe',
  'source-binding.json',
];

async function regular(path, directory = false) {
  try {
    const info = await lstat(path);
    return !info.isSymbolicLink() && (directory ? info.isDirectory() : info.isFile());
  } catch {
    return false;
  }
}

/**
 * The binding answer for one development run.
 *
 * `expectedSha256` comes from the Development Release Bundle that this checkout
 * just regenerated, which binds the Core component to the actual debug output.
 * Without it there is nothing to compare a published object against, and the
 * report says exactly that instead of picking one.
 */
export async function inspectDevExecution({ corePath, programData, repositoryRoot }) {
  const binding = await readDevelopmentBinding(repositoryRoot);
  const expectedSha256 = binding.coreSha256;
  const expectedExecutionSha256 = expectedExecution(binding);
  const reviewDirectoryGaps = [];
  for (const name of REVIEW_FILES) {
    if (!(await regular(join(resolve(corePath, '..'), name)))) reviewDirectoryGaps.push(name);
  }
  const resolution = await resolvePublishedCore({
    programData,
    expectedSha256,
    // Core alone is not the whole execution version. The installation's runner and its
    // pinned PowerShell runtime are separate file objects that can be replaced without
    // touching Core, so an installation whose Core matches can still be running an
    // execution version this checkout did not produce. The expected set is built by the
    // same shared helper the Main process uses, so the two entries cannot disagree
    // about which components the question covers.
    expectedExecutionSha256,
  });
  const described = describeCoreBinding(resolution);
  const scan = await readExecutionInstallations(programData);
  return {
    corePath,
    expectedSha256,
    expectedExecutionSha256,
    mismatchedComponents: resolution.mismatchedComponents ?? [],
    unknownComponents: resolution.unknownComponents ?? [],
    bindingSource: binding.source,
    componentRoot: binding.componentRoot,
    missingBootComponents: binding.missingBootComponents,
    unboundComponents: binding.unboundComponents,
    unboundExecutionComponents: binding.unboundExecutionComponents,
    expectedRunnerSha256: binding.executionComponents.runner ?? null,
    reviewDirectoryGaps,
    resolution,
    binding: described,
    installations: scan.installations,
    gapText:
      described.gap === null
        ? null
        : (CORE_BINDING_GAP_TEXT[described.gap] ?? '未分类的绑定缺口。'),
  };
}

/** Human-readable, bounded report. No path is printed for a refused binding. */
export function describeDevExecution(result) {
  const lines = [`[Aura:execution] 开发入口 Core：${result.corePath}`];
  if (result.expectedSha256 === null) {
    lines.push(
      '本次构建没有产出 Core 组件哈希（Development Release Bundle 未绑定核心程序）。受限 Shell 未准备。',
    );
  }
  if (result.binding.state === 'bound') {
    lines.push(
      `已发布对象与本次构建一致：${result.binding.corePath}（安装 ${result.binding.installationId}）。`,
    );
    lines.push('受限 Shell 的可用性仍由 Core 在运行时核对文件身份、所有者、ACL 与执行资源。');
  } else {
    lines.push(`受限 Shell 未准备（${result.binding.gap}）：${result.gapText}`);
    if (result.unknownComponents?.length) {
      lines.push(
        `无法确认期望版本的执行组件：${result.unknownComponents.join(', ')}。先重新构建绑定记录。`,
      );
    }
    if (result.binding.gap === 'execution_version_mismatch') {
      // Name the components, not just "something differs": the runner and the pinned
      // runtime are replaced by different flows, and a person acting on this needs to
      // know which one the installation disagrees about.
      const names = (result.mismatchedComponents ?? []).join(', ');
      lines.push(
        `不一致的执行组件：${names || '未指明'}；本次构建期望 ${JSON.stringify(result.expectedExecutionSha256 ?? {})}。`,
      );
    }
    for (const item of result.installations) {
      if (item.status === 'usable') {
        lines.push(
          `已发布安装 ${item.installationId} 的 Core 哈希是 ${item.declaredSha256}；整套执行版本的匹配结果见上述诊断。`,
        );
      } else if (item.declaredPath) {
        lines.push(
          `安装 ${item.installationId} 状态 ${item.status}；清单绑定的路径不是该安装目录下的 aura-core.exe。`,
        );
      } else {
        lines.push(`安装 ${item.installationId} 状态 ${item.status}，不能作为可启动发布对象。`);
      }
    }
    if (result.installations.length === 0) lines.push('本机未发现已发布的执行安装。');
    lines.push(
      'dev 继续使用上述 debug Core 提供界面与数据能力，不会静默改用旧版本，也不降级为无限制执行。',
    );
  }
  if (result.missingBootComponents.length) {
    lines.push(
      `缺少 Core 启动所需的发布组件：${result.missingBootComponents.join(', ')}。即使发布 Core 与本次构建一致，切换过去也会在启动时失败；先构建这些组件。`,
    );
  } else if (result.componentRoot !== null) {
    lines.push(
      `发布组件根目录：${result.componentRoot}（Sandbox Host / Egress Proxy / Elevated Helper）。`,
    );
  }
  if (result.unboundComponents.length) {
    lines.push(
      `Development Release Bundle 未绑定这些组件（尚无对应产物）：${result.unboundComponents.join(', ')}。`,
    );
  }
  if (result.unboundExecutionComponents.length) {
    lines.push(
      `未绑定这些执行组件：${result.unboundExecutionComponents.join(', ')}；只比较 Core 哈希不足以证明整套执行版本一致。`,
    );
  }
  if (result.reviewDirectoryGaps.length) {
    lines.push(
      `此目录缺少执行发布输入：${result.reviewDirectoryGaps.join(', ')}。不能直接作为 Repair 的 BuildDirectory。`,
    );
  }
  lines.push('构建与启动步骤见 docs/dev/building-and-testing.md；本预检不改安装或 ACL。');
  return lines.join('\n');
}

/**
 * The report shape with nothing established yet.
 *
 * Every reader field is present so the report cannot fail on a missing key: a
 * diagnostic that throws while explaining a failure is worse than no diagnostic.
 */
function unestablishedReport(corePath, gap) {
  return {
    corePath,
    expectedSha256: null,
    expectedExecutionSha256: {},
    mismatchedComponents: [],
    unknownComponents: [],
    bindingSource: 'missing',
    componentRoot: null,
    missingBootComponents: [],
    unboundComponents: [],
    unboundExecutionComponents: [],
    expectedRunnerSha256: null,
    reviewDirectoryGaps: [],
    resolution: { status: gap, installations: [] },
    binding: {
      state: 'unbound',
      gap,
      corePath: null,
      installationId: null,
      declaredSha256: null,
    },
    installations: [],
    gapText: CORE_BINDING_GAP_TEXT[gap] ?? '未分类的绑定缺口。',
  };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const root = resolve(import.meta.dirname, '..');
  const corePath = join(root, 'target', 'debug', 'aura-core.exe');
  const result = await inspectDevExecution({
    corePath,
    programData: process.env.ProgramData ?? process.env.PROGRAMDATA ?? '',
  }).catch(() => unestablishedReport(corePath, 'installation_store_unreadable'));
  console.log(describeDevExecution(result));
}

export { publishedCorePath };
