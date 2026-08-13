import type { AudioAnalysis } from '@/model/domain';
import surface from '@/shared/ui/Surface.module.css';
import styles from './DesignWorkspace.module.css';

export function WorkspaceAnalyze({ analysis }: { analysis: AudioAnalysis | null }) {
  if (!analysis) {
    return (
      <div className={styles.emptyWorkspace}>
        <span className={styles.emptyOrbit}>
          <i />
          <b />
        </span>
        <span className={surface.eyebrow}>ANALYZE WORKSPACE</span>
        <h1>測定して、理解する</h1>
        <p>
          LibraryのRecordingsからProcessed WAVを選ぶと、音量・位相・簡易スペクトルを確認できます。
        </p>
        <small>解析はオフラインで実行され、元の録音ファイルは変更されません。</small>
      </div>
    );
  }
  return (
    <div className={styles.workspaceScroll}>
      <section className={styles.workspaceHeader}>
        <div>
          <span className={surface.eyebrow}>ANALYSIS RESULT</span>
          <h1>{analysis.path.split('\\').pop() ?? 'Audio'}</h1>
        </div>
        <span className={surface.statusTag}>READ ONLY</span>
      </section>
      <section className={`${surface.sectionCard} ${styles.waveformCard}`}>
        <span className={surface.eyebrow}>WAVEFORM</span>
        <div className={styles.waveformAnalysis}>
          {analysis.waveform.map((value, index) => (
            <i key={index} style={{ height: `${Math.max(4, value * 100)}%` }} />
          ))}
        </div>
      </section>
      <section className={styles.analysisGrid}>
        <article className={surface.sectionCard}>
          <span className={surface.eyebrow}>LEVEL</span>
          <h2>{analysis.rmsDb.toFixed(1)} dB RMS</h2>
          <p>
            Peak {analysis.peakDb.toFixed(1)} dBFS · True peak {analysis.truePeakDb.toFixed(1)} dBFS
          </p>
        </article>
        <article className={surface.sectionCard}>
          <span className={surface.eyebrow}>DYNAMICS</span>
          <h2>{analysis.dynamicRangeDb.toFixed(1)} dB</h2>
          <p>{analysis.clippingSamples.toLocaleString()} clipped samples · estimated from PCM</p>
        </article>
        <article className={surface.sectionCard}>
          <span className={surface.eyebrow}>SPECTRUM</span>
          <h2>{analysis.spectrumPeakHz ? `${analysis.spectrumPeakHz.toFixed(1)} Hz` : '—'}</h2>
          <p>簡易スペクトルピーク</p>
        </article>
        <article className={surface.sectionCard}>
          <span className={surface.eyebrow}>PHASE</span>
          <h2>
            {analysis.phaseCorrelation == null ? 'Mono' : analysis.phaseCorrelation.toFixed(3)}
          </h2>
          <p>
            {analysis.phaseCorrelation == null ? 'ステレオ相関なし' : 'Left / Right correlation'}
          </p>
        </article>
        <article className={surface.sectionCard}>
          <span className={surface.eyebrow}>TIMING</span>
          <h2>{(analysis.durationMs / 1000).toFixed(2)} s</h2>
          <p>
            {analysis.sampleRate} Hz · {analysis.channels} ch · {analysis.bitsPerSample} bit
          </p>
        </article>
      </section>
    </div>
  );
}
