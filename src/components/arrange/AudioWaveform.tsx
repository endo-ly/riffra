import type { AudioAnalysis, AudioClip } from '@/lib/domain';
import styles from './WorkspaceArrange.module.css';

export function AudioWaveform({
  analysis,
  clip,
}: {
  analysis?: AudioAnalysis | null;
  clip: AudioClip;
}) {
  if (!analysis?.waveform.length || analysis.samples <= 0) {
    return <span className={styles.waveformPending}>BUILDING WAVEFORM</span>;
  }
  const first = Math.floor((clip.sourceRange.start / analysis.samples) * analysis.waveform.length);
  const last = Math.max(
    first + 1,
    Math.ceil((clip.sourceRange.end / analysis.samples) * analysis.waveform.length),
  );
  const source = analysis.waveform.slice(first, last);
  const sourceFrames = Math.max(1, clip.sourceRange.end - clip.sourceRange.start);
  const cycles = clip.loopEnabled ? Math.max(1, clip.timelineDuration.frames / sourceFrames) : 1;
  const values = Array.from(
    { length: Math.max(source.length, Math.round(source.length * cycles)) },
    (_, index) => source[index % source.length],
  );
  const stride = Math.max(1, Math.ceil(values.length / 240));
  const visible = values.filter((_, index) => index % stride === 0);
  const gain = Math.min(2.5, 10 ** (clip.gainDb / 20));
  const path = visible
    .map((value, index) => {
      const x = visible.length === 1 ? 50 : (index / (visible.length - 1)) * 100;
      const amplitude = Math.min(21, value * gain * 21);
      return `M${x.toFixed(2)} ${(22 - amplitude).toFixed(2)}V${(22 + amplitude).toFixed(2)}`;
    })
    .join('');
  return (
    <svg className={styles.waveform} viewBox="0 0 100 44" preserveAspectRatio="none" aria-hidden>
      <path d={path} />
    </svg>
  );
}
