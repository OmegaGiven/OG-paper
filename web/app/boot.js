// The canvas plus bookmarks, timeline, offline copies and browser autosave.
// (A file, not an inline script, so hosts with a strict CSP can serve the app: the vault does.)
import { start } from './og-web.js';
start({ mode: 'app' });

// Installable app (Add to Home screen): opens with no browser bars.
if ('serviceWorker' in navigator) navigator.serviceWorker.register('sw.js').catch(() => {});
