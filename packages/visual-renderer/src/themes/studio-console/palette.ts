import type { ChordRole, TrackCategory } from '../../generated';

export interface NoteColorStyle {
  readonly fill: string;
  readonly stroke: string;
  readonly glow: string;
  readonly core: string;
  readonly label: string;
}

export interface TrackColorStyle {
  readonly fill: string;
  readonly stroke: string;
  readonly glow: string;
  readonly core: string;
}

/**
 * Centralized visual design tokens for the Studio Console theme.
 *
 * All surface tones, harmonic accents, glow parameters, decay rates, and
 * geometry constants live here so the visual character can be tuned from one
 * place.
 */
export const STUDIO_PALETTE = {
  surface: {
    chassis: '#090c12',
    chassisDeep: '#06080d',
    panel: '#0e131d',
    panelElevated: '#131a28',
    panelRecessed: '#0a0e16',
    card: '#111723',
    cardActive: '#192132',
    slotEmpty: '#0c1018',
    sustainLane: '#080b11',
  },
  border: {
    subtle: 'rgba(182, 204, 240, 0.08)',
    panel: 'rgba(182, 204, 240, 0.13)',
    specular: 'rgba(255, 255, 255, 0.05)',
    active: 'rgba(245, 185, 99, 0.65)',
  },
  text: {
    primary: '#edf2fb',
    secondary: '#9aa8c4',
    muted: '#61708d',
    dim: '#43506b',
  },
  accent: {
    amber: '#f5b963',
    amberBright: '#ffdc9e',
    amberGlow: 'rgba(245, 185, 99, 0.42)',
    cyan: '#4cd7e8',
    cyanBright: '#9ef2fc',
    cyanGlow: 'rgba(76, 215, 232, 0.36)',
  },
  grid: {
    blackKeyBand: 'rgba(4, 6, 11, 0.42)',
    octaveLine: 'rgba(182, 204, 240, 0.16)',
    barLine: 'rgba(182, 204, 240, 0.22)',
    beatLine: 'rgba(182, 204, 240, 0.075)',
    playheadLine: '#ffdc9e',
    playheadGlow: 'rgba(245, 185, 99, 0.28)',
    playheadWash: 'rgba(245, 185, 99, 0.06)',
    sustainFill: 'rgba(76, 215, 232, 0.2)',
    sustainActiveFill: 'rgba(245, 185, 99, 0.34)',
    sustainStroke: 'rgba(76, 215, 232, 0.65)',
    sustainActiveStroke: '#ffdc9e',
  },
  harmony: {
    root: {
      fill: '#f5b963',
      stroke: '#ffe3b3',
      glow: 'rgba(245, 185, 99, 0.62)',
      core: '#fff4dc',
      label: 'Root',
    },
    chordTone: {
      fill: '#4cd7e8',
      stroke: '#a8f3fc',
      glow: 'rgba(76, 215, 232, 0.56)',
      core: '#e2fcff',
      label: 'Chord',
    },
    tension: {
      fill: '#b58cff',
      stroke: '#dec9ff',
      glow: 'rgba(181, 140, 255, 0.56)',
      core: '#f4ecff',
      label: 'Tension',
    },
    nonChord: {
      fill: '#6f81a4',
      stroke: '#a4b5d6',
      glow: 'rgba(111, 129, 164, 0.42)',
      core: '#dce5f7',
      label: 'Passing',
    },
    fallback: {
      fill: '#5bb8d4',
      stroke: '#9be0f5',
      glow: 'rgba(91, 184, 212, 0.48)',
      core: '#dff7ff',
      label: 'Note',
    },
  } satisfies Record<ChordRole | 'fallback', NoteColorStyle>,
  velocityStops: [
    { at: 0, rgb: [66, 98, 152] as const },
    { at: 0.4, rgb: [72, 186, 214] as const },
    { at: 0.72, rgb: [242, 178, 86] as const },
    { at: 1, rgb: [255, 114, 126] as const },
  ],
  velocityLegend: [
    { label: 'Soft', color: '#426298' },
    { label: 'Medium', color: '#48bad6' },
    { label: 'Strong', color: '#f2b256' },
    { label: 'Peak', color: '#ff727e' },
  ],
  tracks: [
    {
      fill: '#4cd7e8',
      stroke: '#a3f2fc',
      glow: 'rgba(76, 215, 232, 0.54)',
      core: '#e3fcff',
    },
    {
      fill: '#f5b963',
      stroke: '#ffe1b0',
      glow: 'rgba(245, 185, 99, 0.56)',
      core: '#fff3db',
    },
    {
      fill: '#b58cff',
      stroke: '#dfc9ff',
      glow: 'rgba(181, 140, 255, 0.54)',
      core: '#f3ebff',
    },
    {
      fill: '#53d8a8',
      stroke: '#a7f3d7',
      glow: 'rgba(83, 216, 168, 0.54)',
      core: '#e3fff4',
    },
    {
      fill: '#f46d86',
      stroke: '#ffb2c0',
      glow: 'rgba(244, 109, 134, 0.56)',
      core: '#ffe6eb',
    },
    {
      fill: '#7aa4ff',
      stroke: '#bed2ff',
      glow: 'rgba(122, 164, 255, 0.54)',
      core: '#eaf1ff',
    },
    {
      fill: '#e8915a',
      stroke: '#ffc7a3',
      glow: 'rgba(232, 145, 90, 0.54)',
      core: '#fff0e5',
    },
    {
      fill: '#9bd85e',
      stroke: '#cdf2a7',
      glow: 'rgba(155, 216, 94, 0.54)',
      core: '#f2ffe3',
    },
  ] satisfies readonly TrackColorStyle[],
  categoryAlpha: {
    vocal: 0.96,
    lead: 0.94,
    bass: 0.9,
    arp: 0.86,
    chords: 0.72,
    drums: 0.8,
  } satisfies Record<TrackCategory, number>,
  circle: {
    viewBoxSize: 280,
    center: 140,
    outerRingRadius: 118,
    nodeOrbitRadius: 102,
    diatonicArcRadius: 76,
    trailRadius: 74,
    nodeRadius: 15,
    tonicNodeRadius: 17,
    arcStrokeWidth: 8,
    arcGlowWidth: 16,
    trailWidthMin: 1.5,
    trailWidthMax: 3.4,
    diatonicArcColor: 'rgba(76, 215, 232, 0.32)',
    diatonicArcGlow: 'rgba(76, 215, 232, 0.16)',
    diatonicNodeFill: 'rgba(76, 215, 232, 0.18)',
    diatonicNodeStroke: 'rgba(76, 215, 232, 0.65)',
    tonicNodeFill: 'rgba(245, 185, 99, 0.24)',
    tonicNodeStroke: '#f5b963',
    activeRootStroke: '#ffdc9e',
    activeRootFill: 'rgba(245, 185, 99, 0.34)',
    inactiveNodeFill: '#101521',
    inactiveNodeStroke: 'rgba(182, 204, 240, 0.16)',
    trailColor: '#f5b963',
    trailGlow: 'rgba(245, 185, 99, 0.45)',
  },
  pianoRoll: {
    windowSec: 8,
    playheadRatio: 0.28,
    pitchPadding: 4,
    minPitchSpan: 18,
    defaultMinMidi: 48,
    defaultMaxMidi: 84,
    sustainLaneHeight: 28,
    headerInsetTop: 18,
    noteCornerRadius: 3,
    soloVelocityThreshold: 104,
  },
  decay: {
    attackGlowRate: 5.5,
    afterglowRate: 3.8,
    modulationDurationSec: 0.65,
    pulseBounceDurationSec: 0.22,
  },
  typography: {
    sans: "'Inter', 'SF Pro Display', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif",
    mono: "'JetBrains Mono', 'SF Mono', 'Cascadia Code', ui-monospace, monospace",
  },
} as const;

export function interpolateVelocityColor(velocity: number): TrackColorStyle {
  const ratio = Math.min(1, Math.max(0, velocity / 127));
  const stops: readonly { readonly at: number; readonly rgb: readonly [number, number, number] }[] =
    STUDIO_PALETTE.velocityStops;
  let lower = stops[0];
  let upper = stops[stops.length - 1];
  for (let index = 0; index < stops.length - 1; index += 1) {
    if (ratio >= stops[index].at && ratio <= stops[index + 1].at) {
      lower = stops[index];
      upper = stops[index + 1];
      break;
    }
  }
  const span = Math.max(0.0001, upper.at - lower.at);
  const local = (ratio - lower.at) / span;
  const r = Math.round(lower.rgb[0] + (upper.rgb[0] - lower.rgb[0]) * local);
  const g = Math.round(lower.rgb[1] + (upper.rgb[1] - lower.rgb[1]) * local);
  const b = Math.round(lower.rgb[2] + (upper.rgb[2] - lower.rgb[2]) * local);
  const sr = Math.min(255, r + 52);
  const sg = Math.min(255, g + 52);
  const sb = Math.min(255, b + 52);
  return {
    fill: `rgb(${r}, ${g}, ${b})`,
    stroke: `rgb(${sr}, ${sg}, ${sb})`,
    glow: `rgba(${r}, ${g}, ${b}, 0.58)`,
    core: `rgb(${Math.min(255, r + 96)}, ${Math.min(255, g + 96)}, ${Math.min(255, b + 96)})`,
  };
}
