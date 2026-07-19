import React from 'react';
import ReactDOM from 'react-dom/client';
import { getCurrentWindow } from '@tauri-apps/api/window';
import App from './App';
import { RegionSelectOverlay } from './view/RegionSelectOverlay';

/**
 * Entry point for the React application.
 *
 * Tauri opens a second webview (label `region-select`, created at runtime by
 * `start_region_selection` in lib.rs) reusing the same `index.html`/bundle —
 * there's no separate build target for it. Routing on the window label here
 * is the whole "second app": the drag-to-select overlay never touches any of
 * the main chat state, so it doesn't need its own entry point or window
 * definition in `tauri.conf.json`.
 */
const isRegionSelectWindow = getCurrentWindow().label === 'region-select';

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    {isRegionSelectWindow ? <RegionSelectOverlay /> : <App />}
  </React.StrictMode>,
);
