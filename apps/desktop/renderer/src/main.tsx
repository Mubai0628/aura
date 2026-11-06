// The rain on the startup screen starts before anything else is evaluated.
import './splash/start-splash';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { App } from './App';
// The owner's fonts (set C) ship with Aura, the pixel face too (styles/base.css);
// `font-src 'self'` allows no other source.
import '@fontsource-variable/inter';
import '@fontsource-variable/noto-sans-sc';
import '@fontsource-variable/noto-serif-sc';
import '@fontsource-variable/jetbrains-mono';
import './styles/index.css';
import './desktop-session.css';
// The composer's look, after the region styles it refines.
import './styles/composer-glass.css';
// Which of the interface's words take the pixel face; last, so it has the final word on them.
import './styles/pixel-type.css';

const rootElement = document.querySelector('#root');
if (!(rootElement instanceof HTMLElement)) {
  throw new Error('Aura renderer root element is missing.');
}

createRoot(rootElement).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
