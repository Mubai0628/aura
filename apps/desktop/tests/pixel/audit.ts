import type { Page } from '@playwright/test';

/** Something on the page that breaks the pixel look, with where it is and what it has. */
export interface PixelFinding {
  what: 'type' | 'radius' | 'shadow' | 'icon' | 'row' | 'switch';
  where: string;
  value: string;
}

/**
 * Every laid-out element of the page, scrolled out of view or not, and its `::before` and
 * `::after`: words in the pixel face are 12px or 24px at weight 400 without tracking, corners are
 * square, shadows have no blur, and a pixel icon is drawn at 12, 16 or 32px. A row with a field in
 * it, a select or a text box, is one strip: the buttons and fields beside it have its height. A
 * switch keeps its own 34 by 18 wherever it is put, never a size a row gives its buttons.
 */
export function pixelAudit(page: Page): Promise<PixelFinding[]> {
  return page.evaluate(() => {
    const findings: {
      what: 'type' | 'radius' | 'shadow' | 'icon' | 'row' | 'switch';
      where: string;
      value: string;
    }[] = [];
    const name = (element: Element) => {
      const classes = [...element.classList].slice(0, 3).join('.');
      return `${element.tagName.toLowerCase()}${classes ? `.${classes}` : ''}`;
    };
    // The element, its parent and grandparent, and the start of its text.
    const where = (element: Element, part = '') => {
      const parent = element.parentElement;
      const above = parent
        ? `${parent.parentElement ? `${name(parent.parentElement)} > ` : ''}${name(parent)} > `
        : '';
      const text = (element.textContent ?? '').trim().replace(/\s+/gu, ' ').slice(0, 24);
      return `${above}${name(element)}${part}${text ? ` "${text}"` : ''}`;
    };
    // A shadow's lengths are x, y, then blur; its colour is taken out first.
    const blurred = (shadow: string) =>
      shadow !== 'none' &&
      shadow
        .replace(/[a-z]+\([^)]*\)/giu, '')
        .split(',')
        .some((one) => Number([...one.matchAll(/(-?[\d.]+)px/gu)][2]?.[1] ?? 0) !== 0);
    const corners = (style: CSSStyleDeclaration) =>
      [
        style.borderTopLeftRadius,
        style.borderTopRightRadius,
        style.borderBottomRightRadius,
        style.borderBottomLeftRadius,
      ].filter((radius) => radius !== '0px');
    const shape = (style: CSSStyleDeclaration, element: Element, part: string) => {
      const round = corners(style);
      if (round.length > 0)
        findings.push({ what: 'radius', where: where(element, part), value: round.join(' ') });
      if (blurred(style.boxShadow))
        findings.push({ what: 'shadow', where: where(element, part), value: style.boxShadow });
    };
    for (const element of document.body.querySelectorAll('*')) {
      const box = element.getBoundingClientRect();
      if (box.width === 0 || box.height === 0) continue;
      const style = getComputedStyle(element);
      if (style.visibility === 'hidden') continue;
      shape(style, element, '');
      for (const part of ['::before', '::after']) {
        const pseudo = getComputedStyle(element, part);
        if (pseudo.content !== 'none' && pseudo.display !== 'none') shape(pseudo, element, part);
      }
      const words = [...element.childNodes].some(
        (node) => node.nodeType === Node.TEXT_NODE && (node.textContent ?? '').trim() !== '',
      );
      if (
        words &&
        style.fontFamily.startsWith('"Fusion Pixel 12"') &&
        (!['12px', '24px'].includes(style.fontSize) ||
          style.fontWeight !== '400' ||
          !['normal', '0px'].includes(style.letterSpacing))
      )
        findings.push({
          what: 'type',
          where: where(element),
          value: `${style.fontSize} ${style.fontWeight} ${style.letterSpacing}`,
        });
      // A box placed at a fractional offset measures a hair off its size, e.g. 15.99998.
      const [width, height] = [box.width, box.height].map((side) => Math.round(side * 100) / 100);
      if (
        element instanceof SVGSVGElement &&
        element.getAttribute('shape-rendering') === 'crispEdges' &&
        (![12, 16, 32].includes(width!) || width !== height)
      )
        findings.push({ what: 'icon', where: where(element), value: `${width}x${height}` });
      if (element.getAttribute('role') === 'switch' && (width !== 34 || height !== 18))
        findings.push({ what: 'switch', where: where(element), value: `${width}x${height}` });
    }
    // Rows: controls whose middles are level, each starting within 24px of the one before it. A
    // switch is a mark beside its words, and a button drawn inside a field is part of that field.
    const NOT_TYPED = ['checkbox', 'radio', 'range', 'color', 'file', 'hidden', 'button', 'submit'];
    const isField = (element: Element) =>
      element instanceof HTMLSelectElement ||
      (element instanceof HTMLInputElement && !NOT_TYPED.includes(element.type));
    const controls = [...document.body.querySelectorAll('button, select, input')]
      .filter((element) => isField(element) || element.getAttribute('role') !== 'switch')
      .filter((element) => !(element instanceof HTMLInputElement) || isField(element))
      .map((element) => ({ element, box: element.getBoundingClientRect() }))
      .filter(
        ({ element, box }) =>
          box.width > 0 && box.height > 0 && getComputedStyle(element).visibility !== 'hidden',
      );
    const inside = (inner: DOMRect, outer: DOMRect) =>
      inner !== outer &&
      inner.left >= outer.left &&
      inner.right <= outer.right &&
      inner.top >= outer.top &&
      inner.bottom <= outer.bottom;
    const placed = controls.filter(
      (one) => !controls.some((other) => isField(other.element) && inside(one.box, other.box)),
    );
    const middle = (box: DOMRect) => box.top + box.height / 2;
    placed.sort((a, b) => middle(a.box) - middle(b.box) || a.box.left - b.box.left);
    const lines: (typeof placed)[] = [];
    for (const one of placed) {
      const line = lines.at(-1);
      if (line && Math.abs(middle(line[0]!.box) - middle(one.box)) <= 2) line.push(one);
      else lines.push([one]);
    }
    for (const line of lines) {
      line.sort((a, b) => a.box.left - b.box.left);
      let strip: typeof line = [];
      const judge = () => {
        const field = strip.find((one) => isField(one.element));
        if (field === undefined) return;
        const height = Math.round(field.box.height * 100) / 100;
        for (const one of strip) {
          const own = Math.round(one.box.height * 100) / 100;
          if (Math.abs(own - height) > 0.5)
            findings.push({
              what: 'row',
              where: where(one.element),
              value: `${own} beside ${height}`,
            });
        }
      };
      for (const one of line) {
        const last = strip.at(-1);
        if (last && one.box.left - last.box.right > 24) {
          judge();
          strip = [];
        }
        strip.push(one);
      }
      judge();
    }
    return findings;
  });
}
