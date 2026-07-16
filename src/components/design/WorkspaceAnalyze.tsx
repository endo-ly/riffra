import type { AudioAnalysis } from '@/lib/domain';

export function WorkspaceAnalyze({ analysis }: { analysis: AudioAnalysis | null }) {
  if (!analysis) {
    return (
      <div className="empty-workspace">
        <span className="empty-orbit orbit-analyze">
          <i />
          <b />
        </span>
        <span className="eyebrow">ANALYZE WORKSPACE</span>
        <h1>測定して、理解する</h1>
        <p>
          LibraryのRecordingsからProcessed WAVを選ぶと、音量・位相・簡易スペクトルを確認できます。
        </p>
        <small>解析はオフラインで実行され、元の録音ファイルは変更されません。</small>
      </div>
    );
  }
  return (
    <div className="workspace-scroll analysis-view">
      <section className="play-header">
        <div>
          <span className="eyebrow">ANALYSIS RESULT</span>
          <h1>{analysis.path.split('\\').pop() ?? 'Audio'}</h1>
        </div>
        <span className="status-tag">READ ONLY</span>
      </section>
      <section className="section-card waveform-card">
        <span className="eyebrow">WAVEFORM</span>
        <div className="waveform-analysis">
          {analysis.waveform.map((value, index) => (
            <i key={index} style={{ height: `${Math.max(4, value * 100)}%` }} />
          ))}
        </div>
      </section>
      <section className="analysis-grid">
        <article className="section-card">
          <span className="eyebrow">LEVEL</span>
          <h2>{analysis.rmsDb.toFixed(1)} dB RMS</h2>
          <p>
            Peak {analysis.peakDb.toFixed(1)} dBFS · True peak {analysis.truePeakDb.toFixed(1)} dBFS
          </p>
        </article>
        <article className="section-card">
          <span className="eyebrow">DYNAMICS</span>
          <h2>{analysis.dynamicRangeDb.toFixed(1)} dB</h2>
          <p>{analysis.clippingSamples.toLocaleString()} clipped samples · estimated from PCM</p>
        </article>
        <article className="section-card">
          <span className="eyebrow">SPECTRUM</span>
          <h2>{analysis.spectrumPeakHz ? `${analysis.spectrumPeakHz.toFixed(1)} Hz` : '—'}</h2>
          <p>簡易スペクトルピーク</p>
        </article>
        <article className="section-card">
          <span className="eyebrow">PHASE</span>
          <h2>
            {analysis.phaseCorrelation == null ? 'Mono' : analysis.phaseCorrelation.toFixed(3)}
          </h2>
          <p>
            {analysis.phaseCorrelation == null ? 'ステレオ相関なし' : 'Left / Right correlation'}
          </p>
        </article>
        <article className="section-card">
          <span className="eyebrow">TIMING</span>
          <h2>{(analysis.durationMs / 1000).toFixed(2)} s</h2>
          <p>
            {analysis.sampleRate} Hz · {analysis.channels} ch · {analysis.bitsPerSample} bit
          </p>
        </article>
      </section>
    </div>
  );
}
