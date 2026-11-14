/** Whether Aura can be given this tab: an ordinary web page, not a browser or store page. */
export function shareable(url: string): boolean {
  if (!/^https?:\/\//iu.test(url)) return false;
  const host =
    url
      .replace(/^https?:\/\//iu, '')
      .split(/[/?#:]/u)[0]
      ?.toLowerCase() ?? '';
  return host !== 'chrome.google.com' && host !== 'chromewebstore.google.com';
}
