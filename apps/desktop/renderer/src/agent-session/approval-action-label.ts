/**
 * What an approval asks to do, in words: the card's header named the action by its code,
 * `shell.powershell`, which tells a person nothing. An action without a word keeps its code.
 */
const LABELS: Record<string, string> = {
  'fs.list': '列出目录',
  'fs.read': '读取文件',
  'fs.search': '搜索文本',
  'fs.find': '查找文件',
  'fs.write': '写入文件',
  'fs.patch': '修改文件',
  'fs.delete': '删除文件',
  'fs.move': '移动文件',
  'shell.powershell': '运行命令',
  'shell.cmd': '运行命令',
  'terminal.open': '打开终端',
  'terminal.write': '在终端输入',
  'web.fetch': '读取网页',
  'web.read': '读取网页',
  'web.search': '搜索网页',
  'http.request': '发送网络请求',
  'http.download': '下载文件',
  'git.commit': '提交改动',
  'git.branch': '切换或新建分支',
  'git.sync': '同步远程仓库',
  'git.push': '推送到远程仓库',
  'commands.save': '保存命令',
  'commands.delete': '删除命令',
  'workspace_init.complete': '初始化工作区',
  'chrome.open': '在 Chrome 打开网页',
  'chrome.navigate': 'Chrome 标签页跳转',
  'chrome.snapshot': '读取 Chrome 标签页',
  'chrome.screenshot': 'Chrome 标签页截图',
  'chrome.click': '在 Chrome 中点击',
  'chrome.type': '在 Chrome 中输入',
  'chrome.press': '在 Chrome 中按键',
  'chrome.scroll': '滚动 Chrome 标签页',
  'chrome.hover': '在 Chrome 中悬停',
  'chrome.select': '在 Chrome 中选择',
  'chrome.wait': '等待 Chrome 标签页',
  'chrome.console': '读取 Chrome 控制台',
  'chrome.close': '关闭 Chrome 标签页',
};

export function approvalActionLabel(actionKind: string): string {
  if (LABELS[actionKind]) return LABELS[actionKind];
  if (actionKind.startsWith('memory.')) return '修改记忆';
  if (actionKind.startsWith('mcp.')) return '调用 MCP 工具';
  return actionKind;
}
