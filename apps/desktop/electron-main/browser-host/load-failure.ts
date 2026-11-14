/**
 * Why a page did not open, in words, with what to do next. Chromium reports only its network
 * error code, which is kept at the end for whoever looks it up.
 */
export function loadFailureText(code: number, description: string): string {
  return `${loadFailureReason(code)}（${description}）`;
}

function loadFailureReason(code: number): string {
  // Certificate errors are Chromium's -200 to -299.
  if (code <= -200 && code > -300)
    return '这个网站的安全证书有问题，Aura 没有打开它。请确认网址是否正确。';
  switch (code) {
    case -105:
    case -137:
      return '找不到这个网站（域名解析失败），页面没有打开。请检查网址，或检查网络和代理后点刷新。';
    case -106:
      return '电脑当前没有联网，页面没有打开。联网后点刷新。';
    case -21:
      return '网络刚刚发生了变化，页面没有打开。点刷新再试。';
    case -102:
      return '网站拒绝了连接（对方的服务没有运行或端口不对），页面没有打开。确认网址和端口后点刷新。';
    case -7:
    case -109:
    case -118:
      return '连接这个网站超时，页面没有打开。请检查网络和代理后点刷新。';
    case -100:
    case -101:
    case -103:
    case -104:
      return '连接被中断，页面没有打开。点刷新再试；仍不行时检查网络和代理。';
    case -111:
    case -115:
    case -120:
    case -121:
    case -127:
    case -130:
    case -131:
    case -136:
      return '代理连不上，页面没有打开。请检查代理设置后点刷新。';
    case -300:
      return '网址的格式不对，页面没有打开。请检查网址。';
    case -301:
    case -302:
      return '这类网址内置浏览器打不开。';
    case -6:
      return '找不到这个文件，页面没有打开。请检查路径。';
    case -10:
      return '没有权限读取这个地址，页面没有打开。';
    case -310:
      return '网站的跳转太多（可能在来回跳转），页面没有打开。';
    case -324:
      return '网站没有返回任何内容，页面没有打开。点刷新再试。';
    default:
      return '页面没有打开。点刷新再试；仍不行时检查网址、网络和代理。';
  }
}
