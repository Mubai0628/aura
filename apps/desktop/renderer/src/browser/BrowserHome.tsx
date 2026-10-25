import { useEffect, useId, useRef, useState, type FormEvent } from 'react';

import { AppIcon } from '../app-icons';
import './browser-home.css';

/**
 * The browser's
 * start page, for a new page, the Home button and a browser with no page open. `onOpen` returns
 * why an address was not opened.
 */
export function BrowserHome({
  onOpen,
  focus = false,
}: {
  onOpen: (input: string) => Promise<string | undefined>;
  /** The user just asked for this page: the address field takes the keyboard. */
  focus?: boolean;
}) {
  const id = useId();
  const field = useRef<HTMLInputElement>(null);
  const [input, setInput] = useState('');
  const [problem, setProblem] = useState('');
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (focus) field.current?.focus();
  }, [focus]);
  const submit = (event: FormEvent) => {
    event.preventDefault();
    const value = input.trim();
    if (value === '' || busy) return;
    setBusy(true);
    void onOpen(value).then((refused) => {
      setBusy(false);
      setProblem(refused ?? '');
    });
  };
  return (
    <div className="browser-home">
      <HomeArt />
      <form className="browser-home-address field-frame" onSubmit={submit}>
        <AppIcon name="search" size={15} />
        <label className="sr-only" htmlFor={id}>
          打开网址
        </label>
        <input
          autoComplete="off"
          id={id}
          onChange={(event) => {
            setInput(event.target.value);
            setProblem('');
          }}
          placeholder="输入网址"
          ref={field}
          spellCheck={false}
          value={input}
        />
        <button
          aria-label="打开"
          className="icon-button"
          disabled={input.trim() === '' || busy}
          type="submit"
        >
          <AppIcon name="arrow-right" size={15} />
        </button>
      </form>
      {problem === '' ? null : (
        <p className="browser-home-problem" role="alert">
          {problem}
        </p>
      )}
    </div>
  );
}

/** A small browser window drifting beside the globe. */
function Card({ x, y, tone }: { x: number; y: number; tone: 'a' | 'b' | 'c' }) {
  return (
    <g className={`art-card art-card-${tone}`} transform={`translate(${x} ${y})`}>
      <g className="art-card-float">
        <rect className="art-card-body" height="44" rx="6" width="62" />
        <path className="art-card-bar" d="M0 6a6 6 0 0 1 6-6h50a6 6 0 0 1 6 6v5H0z" />
        <circle className="art-card-dot" cx="7" cy="5.5" r="1.6" />
        <circle className="art-card-dot" cx="12" cy="5.5" r="1.6" />
        <circle className="art-card-dot" cx="17" cy="5.5" r="1.6" />
        <rect className="art-card-line" height="3" rx="1.5" width="40" x="8" y="19" />
        <rect className="art-card-line" height="3" rx="1.5" width="24" x="8" y="27" />
        <rect className="art-card-block" height="9" rx="2" width="14" x="40" y="26" />
      </g>
    </g>
  );
}

/** A globe with drifting land, an orbit and a few pages around it, in the window's colours. */
function HomeArt() {
  const id = useId().replace(/[^\w-]/gu, '');
  const halo = `${id}-halo`;
  const sphere = `${id}-sphere`;
  const clip = `${id}-clip`;
  const land = `${id}-land`;
  return (
    <svg aria-hidden="true" className="browser-home-art" focusable="false" viewBox="0 0 320 210">
      <defs>
        <radialGradient id={halo}>
          <stop className="art-halo-in" offset="0" />
          <stop className="art-halo-out" offset="1" />
        </radialGradient>
        <linearGradient id={sphere} x1="0" x2="0" y1="0" y2="1">
          <stop className="art-sphere-top" offset="0" />
          <stop className="art-sphere-bottom" offset="1" />
        </linearGradient>
        <clipPath id={clip}>
          <circle cx="160" cy="108" r="54" />
        </clipPath>
        <g id={land}>
          <path
            className="art-land-shape"
            d="M118 92c8-10 22-12 30-6s6 16-2 22-20 10-26 4-8-12-2-20z"
          />
          <path
            className="art-land-shape"
            d="M168 118c10-6 26-4 32 6s-2 22-14 24-24-2-26-12 0-14 8-18z"
          />
          <path className="art-land-shape" d="M176 70c6-3 14-1 15 4s-5 8-11 7-9-8-4-11z" />
        </g>
      </defs>
      <circle cx="160" cy="108" fill={`url(#${halo})`} r="104" />
      <ellipse
        className="art-orbit"
        cx="160"
        cy="108"
        rx="118"
        ry="30"
        transform="rotate(-12 160 108)"
      />
      <circle cx="160" cy="108" fill={`url(#${sphere})`} r="54" />
      <g clipPath={`url(#${clip})`}>
        <g className="art-land">
          <use href={`#${land}`} />
          <use href={`#${land}`} x="-108" />
        </g>
        <g className="art-grid">
          <ellipse cx="160" cy="108" rx="20" ry="54" />
          <ellipse cx="160" cy="108" rx="40" ry="54" />
          <ellipse cx="160" cy="108" rx="54" ry="14" />
          <path d="M106 84q54-14 108 0M106 132q54 14 108 0" />
        </g>
      </g>
      <circle className="art-rim" cx="160" cy="108" r="54" />
      <path className="art-orbit" d="M42 108a118 30 0 0 0 236 0" transform="rotate(-12 160 108)" />
      <circle className="art-planet" cx="275" cy="84" r="4.5" />
      <path className="art-spark" d="M262 28l2 6 6 2-6 2-2 6-2-6-6-2 6-2z" />
      <Card tone="a" x={30} y={30} />
      <Card tone="b" x={228} y={112} />
      <Card tone="c" x={56} y={146} />
    </svg>
  );
}
