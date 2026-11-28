import { STUDIO_PALETTE } from './palette';

const P = STUDIO_PALETTE;

/**
 * Self-contained scoped stylesheet for the Studio Console theme.
 *
 * Intentionally contains no CSS transitions or keyframe animations so every
 * rendered frame is a pure function of `(snapshot, timeSec, state)`.
 */
export const STUDIO_CONSOLE_STYLES = `
.sc-root {
  box-sizing: border-box;
  width: 100%;
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 12px;
  background:
    radial-gradient(ellipse 85% 55% at 22% 0%, rgba(76, 215, 232, 0.065), transparent 65%),
    radial-gradient(ellipse 70% 50% at 86% 18%, rgba(245, 185, 99, 0.06), transparent 62%),
    linear-gradient(180deg, ${P.surface.chassis} 0%, ${P.surface.chassisDeep} 100%);
  color: ${P.text.primary};
  font-family: ${P.typography.sans};
  user-select: none;
  overflow: hidden;
}

.sc-root *,
.sc-root *::before,
.sc-root *::after {
  box-sizing: border-box;
}

.sc-root[data-layout="9:16"] {
  padding: 12px 12px 68px 12px;
  gap: 8px;
}

.sc-panel {
  background: linear-gradient(180deg, ${P.surface.panelElevated} 0%, ${P.surface.panel} 100%);
  border: 1px solid ${P.border.panel};
  border-radius: 10px;
  box-shadow:
    inset 0 1px 0 ${P.border.specular},
    0 8px 24px rgba(0, 0, 0, 0.34);
  overflow: hidden;
}

.sc-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 8px 14px;
  background: linear-gradient(180deg, rgba(22, 29, 44, 0.92) 0%, rgba(14, 19, 29, 0.94) 100%);
  border: 1px solid ${P.border.panel};
  border-radius: 10px;
  box-shadow: inset 0 1px 0 ${P.border.specular};
  flex-shrink: 0;
}

.sc-header[hidden] {
  display: none;
}

.sc-header-brand {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
}

.sc-status-led {
  width: 8px;
  height: 8px;
  border-radius: 999px;
  background: ${P.accent.amber};
  box-shadow: 0 0 10px ${P.accent.amberGlow};
  flex-shrink: 0;
}

.sc-title {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
  letter-spacing: 0.02em;
  color: ${P.text.primary};
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.sc-header-meta {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
  flex-wrap: wrap;
}

.sc-meta-pill {
  display: inline-flex;
  align-items: center;
  padding: 3px 9px;
  border-radius: 6px;
  background: ${P.surface.panelRecessed};
  border: 1px solid ${P.border.subtle};
  font-family: ${P.typography.mono};
  font-size: 11px;
  font-weight: 500;
  letter-spacing: 0.04em;
  color: ${P.text.secondary};
  font-variant-numeric: tabular-nums;
}

.sc-meta-pill[hidden] {
  display: none;
}

.sc-meta-pill--accent {
  color: ${P.accent.amberBright};
  border-color: rgba(245, 185, 99, 0.28);
  background: rgba(245, 185, 99, 0.08);
}

/* Chord Progression Strip */
.sc-chord-strip {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 8px;
  flex-shrink: 0;
}

.sc-chord-strip[hidden] {
  display: none;
}

.sc-root[data-layout="9:16"] .sc-chord-strip {
  grid-template-columns: repeat(2, minmax(0, 1fr));
}

.sc-chord-card {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  text-align: center;
  min-height: 60px;
  padding: 8px 14px 12px 14px;
  border-radius: 9px;
  background: linear-gradient(180deg, ${P.surface.card} 0%, ${P.surface.panelRecessed} 100%);
  border: 1px solid ${P.border.subtle};
  box-shadow: inset 0 1px 0 ${P.border.specular};
  overflow: hidden;
  opacity: 0.72;
}

.sc-chord-card[hidden] {
  display: none;
}

.sc-chord-card[data-active="true"] {
  opacity: 1;
  background: linear-gradient(180deg, #1d263a 0%, #121928 100%);
  border-color: ${P.border.active};
  box-shadow:
    inset 0 1px 0 rgba(255, 224, 170, 0.22),
    0 0 26px rgba(245, 185, 99, 0.18);
}

.sc-chord-primary {
  font-size: 20px;
  font-weight: 700;
  line-height: 1.15;
  letter-spacing: -0.01em;
  color: ${P.text.secondary};
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  max-width: 100%;
}

.sc-chord-card[data-active="true"] .sc-chord-primary {
  font-size: 23px;
  color: ${P.accent.amberBright};
  text-shadow: 0 0 18px rgba(245, 185, 99, 0.42);
}

.sc-chord-secondary {
  margin-top: 2px;
  font-family: ${P.typography.mono};
  font-size: 11px;
  font-weight: 500;
  color: ${P.text.muted};
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  max-width: 100%;
}

.sc-chord-secondary[hidden] {
  display: none;
}

.sc-chord-card[data-active="true"] .sc-chord-secondary {
  color: ${P.text.primary};
}

.sc-chord-progress-track {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  height: 4px;
  background: rgba(182, 204, 240, 0.08);
}

.sc-chord-progress-fill {
  height: 100%;
  width: 0%;
  background: linear-gradient(90deg, ${P.accent.amber} 0%, ${P.accent.amberBright} 100%);
  box-shadow: 0 0 10px ${P.accent.amberGlow};
}

.sc-chord-empty {
  grid-column: 1 / -1;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  min-height: 62px;
  padding: 10px 14px;
  border-radius: 9px;
  background: ${P.surface.slotEmpty};
  border: 1px dashed ${P.border.subtle};
  font-family: ${P.typography.mono};
  font-size: 11px;
  letter-spacing: 0.12em;
  text-transform: uppercase;
  color: ${P.text.muted};
}

.sc-chord-empty[hidden] {
  display: none;
}

/* Main Stage (Piano Roll + Circle of Fifths) */
.sc-main {
  flex: 1 1 0;
  min-height: 0;
  display: grid;
  grid-template-columns: minmax(0, 2.05fr) minmax(240px, 1fr);
  gap: 10px;
}

.sc-root[data-layout="9:16"] .sc-main {
  grid-template-columns: minmax(0, 1fr);
  grid-template-rows: minmax(0, 1.35fr) minmax(200px, 0.95fr);
}

/* Shared Panel Chrome */
.sc-panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 5px 11px;
  border-bottom: 1px solid ${P.border.subtle};
  background: rgba(9, 13, 21, 0.42);
  flex-shrink: 0;
}

.sc-panel-title {
  font-family: ${P.typography.mono};
  font-size: 9.5px;
  font-weight: 600;
  letter-spacing: 0.14em;
  text-transform: uppercase;
  color: ${P.text.muted};
}

.sc-panel-tag {
  font-family: ${P.typography.mono};
  font-size: 10px;
  font-weight: 600;
  letter-spacing: 0.06em;
  color: ${P.accent.amberBright};
  font-variant-numeric: tabular-nums;
}

.sc-panel-tag[hidden] {
  display: none;
}

/* Piano Roll */
.sc-piano-panel {
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.sc-legend {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
  justify-content: flex-end;
}

.sc-legend-item {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-family: ${P.typography.mono};
  font-size: 10px;
  color: ${P.text.secondary};
}

.sc-legend-item[hidden] {
  display: none;
}

.sc-legend-swatch {
  width: 8px;
  height: 8px;
  border-radius: 2px;
  flex-shrink: 0;
}

.sc-piano-canvas-wrap {
  position: relative;
  flex: 1 1 0;
  min-height: 120px;
  background: ${P.surface.panelRecessed};
}

.sc-piano-canvas {
  display: block;
  width: 100%;
  height: 100%;
}

.sc-piano-canvas[data-seekable="true"] {
  cursor: pointer;
}

/* Circle of Fifths */
.sc-circle-panel {
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.sc-circle-stage {
  flex: 1 1 0;
  min-height: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 10px 14px;
}

.sc-circle-svg {
  width: 100%;
  height: 100%;
  max-width: 380px;
  max-height: 380px;
  overflow: visible;
}

.sc-circle-footer {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 6px 12px;
  border-top: 1px solid ${P.border.subtle};
  background: rgba(9, 13, 21, 0.42);
  font-family: ${P.typography.mono};
  font-size: 11px;
  color: ${P.text.secondary};
  font-variant-numeric: tabular-nums;
  flex-shrink: 0;
}

.sc-circle-footer[hidden] {
  display: none;
}

.sc-circle-key {
  color: ${P.accent.cyanBright};
  font-weight: 600;
  letter-spacing: 0.04em;
}

/* Pulse Lane */
.sc-pulse-panel {
  display: flex;
  flex-direction: column;
  flex-shrink: 0;
  max-height: 28%;
}

.sc-pulse-panel[hidden] {
  display: none;
}

.sc-pulse-body {
  display: flex;
  flex-direction: column;
  gap: 5px;
  padding: 8px 12px 10px 12px;
  overflow-y: auto;
}

.sc-pulse-panel[data-compact="true"] .sc-pulse-body {
  gap: 3px;
  padding: 6px 12px 8px 12px;
}

.sc-pulse-row {
  display: grid;
  grid-template-columns: 88px minmax(0, 1fr);
  align-items: center;
  gap: 10px;
}

.sc-root[data-layout="9:16"] .sc-pulse-row {
  grid-template-columns: 64px minmax(0, 1fr);
  gap: 6px;
}

.sc-pulse-label {
  font-family: ${P.typography.mono};
  font-size: 10px;
  font-weight: 600;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: ${P.text.secondary};
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.sc-pulse-steps {
  display: grid;
  gap: 4px;
  align-items: center;
}

.sc-root[data-layout="9:16"] .sc-pulse-steps {
  gap: 2px;
}

.sc-pulse-step {
  position: relative;
  min-width: 0;
  height: 18px;
  padding: 3px;
  border-radius: 4px;
  background: rgba(14, 20, 32, 0.85);
  border: 1px solid ${P.border.subtle};
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
}

.sc-pulse-panel[data-compact="true"] .sc-pulse-step {
  height: 13px;
  padding: 2px;
}

.sc-pulse-step[data-beat-group="odd"] {
  background: rgba(20, 28, 44, 0.85);
}

.sc-pulse-step[data-beat-start="true"] {
  border-left: 2px solid rgba(182, 204, 240, 0.28);
}

.sc-pulse-step[data-current="true"] {
  background: rgba(245, 185, 99, 0.18);
  border-color: rgba(245, 185, 99, 0.55);
}

.sc-pulse-dot {
  width: 4px;
  height: 4px;
  border-radius: 999px;
  background: rgba(182, 204, 240, 0.14);
}

.sc-pulse-step[data-hit="regular"] .sc-pulse-dot {
  width: 100%;
  height: 100%;
  border-radius: 2px;
  background: linear-gradient(180deg, ${P.accent.cyanBright} 0%, ${P.accent.cyan} 100%);
  box-shadow: 0 0 8px ${P.accent.cyanGlow};
}

.sc-pulse-step[data-hit="accent"] .sc-pulse-dot {
  width: 100%;
  height: 100%;
  border-radius: 2px;
  background: linear-gradient(180deg, ${P.accent.amberBright} 0%, ${P.accent.amber} 100%);
  box-shadow: 0 0 11px ${P.accent.amberGlow};
}

.sc-pulse-empty {
  padding: 10px 0;
  text-align: center;
  font-family: ${P.typography.mono};
  font-size: 11px;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: ${P.text.muted};
}

.sc-pulse-empty[hidden] {
  display: none;
}
`;
