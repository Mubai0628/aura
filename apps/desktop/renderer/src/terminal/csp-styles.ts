// The packaged page's CSP (`style-src 'self'`) blocks inline <style> elements. xterm's DOM
// renderer and viewport write their font, cell, cursor, selection, ANSI color and scrollbar
// rules into such elements directly under `.xterm-screen`, so the browser ignores them and the
// terminal renders in the app font with no cursor or colors. A constructed stylesheet is not an
// inline style: mirror each element's text into one adopted by the document. The blocked
// originals still report CSP violations, which is expected.
interface Mirror {
  sheet: CSSStyleSheet;
  text: string | null;
}

export function adoptTerminalStyles(screen: Element): () => void {
  const mirrored = new Map<HTMLStyleElement, Mirror>();
  const adopt = (update: (sheets: CSSStyleSheet[]) => CSSStyleSheet[]) => {
    document.adoptedStyleSheets = update([...document.adoptedStyleSheets]);
  };
  // xterm rewrites an element's text when the theme or cell size changes.
  const textObserver = new MutationObserver(() => sync());
  // Only direct children: row updates happen deeper and must not run this on every frame.
  const childObserver = new MutationObserver(() => sync());
  const sync = () => {
    const present = new Set(
      [...screen.children].filter(
        (child): child is HTMLStyleElement => child instanceof HTMLStyleElement,
      ),
    );
    for (const [element, mirror] of mirrored) {
      if (present.has(element)) continue;
      mirrored.delete(element);
      adopt((sheets) => sheets.filter((sheet) => sheet !== mirror.sheet));
    }
    for (const element of present) {
      let mirror = mirrored.get(element);
      if (!mirror) {
        const created: Mirror = { sheet: new CSSStyleSheet(), text: null };
        mirrored.set(element, created);
        textObserver.observe(element, { childList: true, characterData: true, subtree: true });
        adopt((sheets) => [...sheets, created.sheet]);
        mirror = created;
      }
      const text = element.textContent ?? '';
      if (mirror.text === text) continue;
      mirror.sheet.replaceSync(text);
      mirror.text = text;
    }
  };
  childObserver.observe(screen, { childList: true });
  sync();
  return () => {
    textObserver.disconnect();
    childObserver.disconnect();
    const owned = new Set([...mirrored.values()].map((mirror) => mirror.sheet));
    adopt((sheets) => sheets.filter((sheet) => !owned.has(sheet)));
    mirrored.clear();
  };
}
