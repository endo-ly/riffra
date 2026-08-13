import { useMemo } from 'react';
import type { AudioAnalysis, AudioClip } from '@/model/domain';
import styles from '../WorkspaceArrange.module.css';

export function AudioWaveform({
  analysis,
  clip,
}: {
  analysis?: AudioAnalysis | null;
  clip: AudioClip;
}) {
  const path = useMemo(() => {
    if (!analysis?.waveform.length || analysis.samples <= 0) return null;
    const first = Math.floor(
      (clip.sourceRange.start / analysis.samples) * analysis.waveform.length,
    );
    const last = Math.max(
      first + 1,
      Math.ceil((clip.sourceRange.end / analysis.samples) * analysis.waveform.length),
    );
    const source = analysis.waveform.slice(first, last);
    const sourceFrames = Math.max(1, clip.sourceRange.end - clip.sourceRange.start);
    const cycles = clip.loopEnabled ? Math.max(1, clip.timelineDuration.frames / sourceFrames) : 1;
    const totalPoints = Math.max(source.length, Math.round(source.length * cycles));
    const stride = Math.max(1, Math.ceil(totalPoints / 240));
    const gain = Math.min(2.5, 10 ** (clip.gainDb / 20));
    // A looped clip can span hundreds of source repetitions. Materializing
    // `source.length * cycles` values would allocate an unbounded array for
    // every render; sampling directly keeps the work bounded regardless of
    // how far the loop was stretched.
    const visible: number[] = [];
    for (let index = 0; index < totalPoints && visible.length < 240; index += stride) {
      visible.push(source[index % source.length]);
    }
    return visible
      .map((value, index) => {
        const x = visible.length === 1 ? 50 : (index / (visible.length - 1)) * 100;
        const amplitude = Math.min(21, value * gain * 21);
        return `M${x.toFixed(2)} ${(22 - amplitude).toFixed(2)}V${(22 + amplitude).toFixed(2)}`;
      })
      .join('');
  }, [analysis, clip.gainDb, clip.loopEnabled, clip.sourceRange, clip.timelineDuration]);
  if (path == null) {
    return <span className={styles.waveformPending}>BUILDING WAVEFORM</span>;
  }
  return (
    <svg className={styles.waveform} viewBox="0 0 100 44" preserveAspectRatio="none" aria-hidden>
      <path d={path} />
    </svg>
  );
}
