// The one door between the app and the new shell. src/main.tsx imports its App
// from here: `?shell=new` opens the shell, anything else today's app.
//
// Only the interface asked for is loaded. Today's app reaches for Tauri while
// its modules load, so importing it at all stops the shell from running in a
// plain browser against the contract fake; the shell needs nothing of it.

// `npm run start:shell` sets VITE_SHELL=new (scripts/start-shell.mjs); `?shell=` wins over it.
const wanted = (new URLSearchParams(window.location.search).get('shell') ?? import.meta.env.VITE_SHELL) === 'new';

const Root = wanted ? (await import('./Shell')).default : (await import('../App')).default;

export default Root;
