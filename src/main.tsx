import React from 'react';
import { createRoot } from 'react-dom/client';
import { installFrontendLogBridge } from './utils/frontendLogBridge';
import './styles.css';

installFrontendLogBridge();

// Development only: `?shell=new` (or `npm run start:shell`, which sets VITE_SHELL)
// opens the engine test bench in src/shell. Today's app reaches for Tauri while
// its modules load, so only the interface asked for is imported. In a release
// build `import.meta.env.DEV` is false and the bundler drops the bench.
const bench =
  import.meta.env.DEV &&
  (new URLSearchParams(window.location.search).get('shell') ?? import.meta.env.VITE_SHELL) === 'new';
const App = bench ? (await import('./shell/Shell')).default : (await import('./App')).default;

const root = createRoot(document.getElementById('root')!);
root.render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
