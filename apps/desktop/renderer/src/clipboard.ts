/**
 * Main refuses
 * the browser's clipboard permissions to every page, so `navigator.clipboard` always failed. Text
 * the user copies goes to Main instead. Resolves to nothing once the clipboard took it, or to what
 * to tell the user: Main's reason, or `failed` when Main could not be asked.
 */
export async function copyText(text: string, failed: string): Promise<string | undefined> {
  try {
    const result = await window.aura.v1.writeClipboard({ text });
    if (!result.ok) return failed;
    return result.value.completed ? undefined : result.value.message;
  } catch {
    return failed;
  }
}
