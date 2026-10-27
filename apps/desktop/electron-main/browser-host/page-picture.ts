/**
 * A picture of a page of Aura's browser, for the renderer to draw where the page would be while
 * one of Aura's own dialogs or menus is over it: the native page view is above everything the
 * window draws, so the page steps aside for them. A hidden page is painted for the capture and
 * stays hidden.
 */
export interface PicturedContents {
  isDestroyed(): boolean;
  capturePage(
    rect?: undefined,
    options?: { stayHidden?: boolean },
  ): Promise<{ isEmpty(): boolean; toJPEG(quality: number): Buffer }>;
}

/** How long a capture may take; a page that has not painted by then goes without a picture. */
export const CAPTURE_LIMIT_MS = 3_000;

/** The page as a JPEG data URL, or undefined when it is closed, blank or slow to paint. */
export async function pagePicture(
  contents: PicturedContents | undefined,
): Promise<string | undefined> {
  if (contents === undefined || contents.isDestroyed()) return undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    const image = await Promise.race([
      contents.capturePage(undefined, { stayHidden: true }),
      new Promise<undefined>((done) => {
        timer = setTimeout(() => done(undefined), CAPTURE_LIMIT_MS);
      }),
    ]);
    if (image === undefined || image.isEmpty()) return undefined;
    return `data:image/jpeg;base64,${image.toJPEG(80).toString('base64')}`;
  } catch {
    return undefined;
  } finally {
    clearTimeout(timer);
  }
}
