export type RunePage = 'demo' | 'settings' | 'saves';

export interface RuneCaptureState {
  id: string;
  page: RunePage;
  slots: 'empty' | 'filled';
  note: string;
}

export interface RuneCaptureRoute {
  name: string;
  path: string;
  expectedTitle: string;
  states: RuneCaptureState[];
}

export const routes: RuneCaptureRoute[] = [
  {
    name: 'rune-window',
    path: '/',
    expectedTitle: 'GBA synthetic snapshot — Rune prototype',
    states: [
      { id: 'filled-demo', page: 'demo', slots: 'empty', note: 'Built-in synthetic snapshot; no ROM is loaded.' },
      { id: 'filled-settings', page: 'settings', slots: 'empty', note: 'Default settings and host key bindings.' },
      { id: 'empty-saves', page: 'saves', slots: 'empty', note: 'No persisted synthetic save slots.' },
      { id: 'filled-saves', page: 'saves', slots: 'filled', note: 'One exact generated GBANSV01 fixture in localStorage.' },
    ],
  },
];
