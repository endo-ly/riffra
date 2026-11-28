import type { VisualSnapshot } from './generated';

export type ColorMode = 'harmony' | 'velocity' | 'track';

export type VisualLayout = '16:9' | '9:16' | 'free';

/** Presentation choices the host controls without rebuilding the snapshot. */
export interface ViewState {
  colorMode: ColorMode;
  layout?: VisualLayout;
  /** Hides the theme header when the host already shows this information. */
  showHeader?: boolean;
  /** Reports a click position on the time axis so the host can seek. */
  onSeek?: (timeSec: number) => void;
}

/** A self-contained renderer for one snapshot contract. */
export interface VisualTheme {
  readonly id: string;
  readonly name: string;
  readonly description: string;
  readonly supportedLayouts: readonly VisualLayout[];
  mount(container: HTMLElement, snapshot: VisualSnapshot): void;
  updateSnapshot?(snapshot: VisualSnapshot): void;
  render(snapshot: VisualSnapshot, timeSec: number, state: ViewState): void;
  unmount(): void;
}
