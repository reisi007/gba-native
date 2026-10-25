// UI-review route/state manifest. One route (the only page of this foundation),
// eight explicit states, three viewport projects. The generic spec reads this
// file; adding a state is a manifest edit only.

export type UiReviewViewport = 'desktop' | 'mobile-portrait' | 'mobile-landscape';

export interface UiReviewStateSpec {
  /** Directory under WEB_REVIEW_OUT; also the capture file prefix context. */
  id: string;
  /** Query string without the leading `?`. */
  query: string;
  /** Body data-app-state the capture waits for. */
  expectedState: 'empty' | 'loading' | 'ready' | 'error' | 'lost';
  prepare?: 'seed-fallback-notice' | 'deny-restart-storage' | 'oversize-initial';
  note?: string;
}

export interface UiReviewRoute {
  name: string;
  path: string;
  expectedTitle: string;
  states: UiReviewStateSpec[];
  viewports?: UiReviewViewport[];
  leadingSections?: number;
  trailingSections?: number;
  note?: string;
}

export const uiReviewConfig = {
  routes: [
    {
      name: 'foundation',
      path: '/',
      expectedTitle: 'gba-native browser GPU foundation (synthetic only)',
      viewports: ['desktop', 'mobile-portrait', 'mobile-landscape'] as UiReviewViewport[],
      leadingSections: 4,
      trailingSections: 2,
      note: 'Existing states retain the real WebGPU/WebGL2 paths, invalid-backend validation and real WebGL context loss. Additional lifecycle captures seed only warning text, deny storage, or apply an oversized initial CSS surface.',
      states: [
        { id: 'empty', query: '', expectedState: 'empty', note: 'No execution before Start.' },
        {
          id: 'filled-webgpu',
          query: 'autostart=1&backend=webgpu',
          expectedState: 'ready',
          note: 'Forced WebGPU. Adapter identity may be redacted by the browser.',
        },
        {
          id: 'filled-webgl2',
          query: 'autostart=1&backend=webgl2',
          expectedState: 'ready',
          note: 'Forced WebGL2 compatibility path.',
        },
        {
          id: 'error-invalid-backend',
          query: 'autostart=1&backend=canvas2d',
          expectedState: 'error',
          note: 'Unknown backend query is rejected before any context; no Canvas2D fallback exists.',
        },
        {
          id: 'lost-context',
          query: 'autostart=1&backend=webgl2&scene=lose-context',
          expectedState: 'lost',
          note: 'Real WEBGL_lose_context event; rendering halts and Restart is offered.',
        },
        {
          id: 'fallback-webgl2-ready',
          query: 'autostart=1&backend=webgl2&fallback=webgpu-init',
          expectedState: 'ready',
          prepare: 'seed-fallback-notice',
          note: 'Capture-only UI fixture seeds a transient WebGPU failure reason; WebGL2 still runs and presents the real synthetic pixels.',
        },
        {
          id: 'storage-restart-blocked',
          query: 'autostart=1&backend=webgl2',
          expectedState: 'ready',
          prepare: 'deny-restart-storage',
          note: 'Real WebGL2 presentation, then storage denial blocks the explicit reload; no retry is triggered.',
        },
        {
          id: 'error-oversize-initial',
          query: 'backend=webgl2',
          expectedState: 'error',
          prepare: 'oversize-initial',
          note: 'The real initial surface-cap guard is reached before GPU initialization; no canvas pixels are fabricated.',
        },
      ] satisfies UiReviewStateSpec[],
    },
  ] satisfies UiReviewRoute[],
};

export const routes = uiReviewConfig.routes;
