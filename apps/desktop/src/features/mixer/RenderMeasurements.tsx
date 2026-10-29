import type { AudioMeasurements } from '@/model/generated';
import styles from './Mixdown.module.css';

interface RenderMeasurementsProps {
  value: AudioMeasurements;
  warnings?: readonly string[];
}

/** Loudness and peak readouts of one rendered WAV, with the reasons any are missing. */
export function RenderMeasurements({ value, warnings = [] }: RenderMeasurementsProps) {
  const { loudness, sectionLoudnessState } = value;
  return (
    <>
      <dl className={styles.measurements}>
        {loudness.state === 'measured' && (
          <>
            <div>
              <dt>Integrated</dt>
              <dd>{loudness.measurement.integratedLufs.toFixed(2)} LUFS</dd>
            </div>
            <div>
              <dt>True Peak</dt>
              <dd>{loudness.measurement.truePeakDb.toFixed(2)} dBTP</dd>
            </div>
            <div>
              <dt>Loudness Range</dt>
              <dd>{loudness.measurement.loudnessRangeLu.toFixed(2)} LU</dd>
            </div>
          </>
        )}
        <div>
          <dt>Sample Peak</dt>
          <dd>{value.samplePeakDbfs === null ? '−∞' : value.samplePeakDbfs.toFixed(2)} dBFS</dd>
        </div>
        {loudness.state === 'unavailable' && (
          <div className={styles.unavailable}>
            <dt>Loudness unavailable</dt>
            <dd>{loudness.reason}</dd>
          </div>
        )}
        {sectionLoudnessState.state === 'unavailable' && (
          <div className={styles.unavailable}>
            <dt>Section loudness unavailable</dt>
            <dd>{sectionLoudnessState.reason}</dd>
          </div>
        )}
      </dl>
      {warnings.map((warning) => (
        <p key={warning} className={styles.warning} role="alert">
          {warning}
        </p>
      ))}
    </>
  );
}
