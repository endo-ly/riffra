import { useEffect, useRef, useState } from 'react';
import type {
  AudioMeasurements,
  BackgroundJobStatus,
  CanonicalState,
  CreativeSession,
  LoudnessMastering,
  MixdownSettings,
  RenderRange,
} from '@/model/domain';
import type { ArrangeWorkspaceApi } from '@/features/arrange/arrange-api';
import { RenderHistoryPanel } from './RenderHistoryPanel';
import { chooseRenderStemDirectory, saveRenderWav } from '@/native/dialog';
import styles from './Mixer.module.css';

interface Props {
  session: CreativeSession;
  api: ArrangeWorkspaceApi;
  applyCanonicalState: (state: CanonicalState) => boolean;
  onClose: () => void;
}

function sameMixdownSettings(left: MixdownSettings, right: MixdownSettings) {
  const sameMastering =
    left.mastering === null || right.mastering === null
      ? left.mastering === right.mastering
      : left.mastering.integratedLufs === right.mastering.integratedLufs &&
        left.mastering.truePeakDb === right.mastering.truePeakDb &&
        left.mastering.loudnessRangeLu === right.mastering.loudnessRangeLu;
  return (
    left.musicalEndTick === right.musicalEndTick &&
    left.tailSeconds === right.tailSeconds &&
    left.fadeOutSeconds === right.fadeOutSeconds &&
    left.sampleRate === right.sampleRate &&
    left.blockSize === right.blockSize &&
    sameMastering
  );
}

function Measurements({
  value,
  warnings,
}: {
  value: AudioMeasurements;
  warnings: readonly string[];
}) {
  return (
    <>
      <dl className={styles.measurements}>
        <dt>Sample Peak</dt>
        <dd>{value.samplePeakDbfs === null ? '−∞' : value.samplePeakDbfs.toFixed(2)} dBFS</dd>
        {value.loudness.state === 'measured' ? (
          <>
            <dt>True Peak</dt>
            <dd>{value.loudness.measurement.truePeakDb.toFixed(2)} dBTP</dd>
            <dt>Integrated Loudness</dt>
            <dd>{value.loudness.measurement.integratedLufs.toFixed(2)} LUFS</dd>
            <dt>Loudness Range</dt>
            <dd>{value.loudness.measurement.loudnessRangeLu.toFixed(2)} LU</dd>
          </>
        ) : (
          <>
            <dt>Loudness measurement</dt>
            <dd>{value.loudness.reason}</dd>
          </>
        )}
        {value.sectionLoudnessState.state === 'unavailable' && (
          <>
            <dt>Section loudness measurement</dt>
            <dd>{value.sectionLoudnessState.reason}</dd>
          </>
        )}
      </dl>
      {warnings.map((warning) => (
        <p key={warning} role="alert">
          {warning}
        </p>
      ))}
    </>
  );
}

export function MixdownDialog(props: Props) {
  const canonical = props.session.settings.mixdown;
  const [enabled, setEnabled] = useState(canonical.mastering !== null);
  const [target, setTarget] = useState<LoudnessMastering>(
    canonical.mastering ?? { integratedLufs: -14, truePeakDb: -1, loudnessRangeLu: 11 },
  );
  const [tail, setTail] = useState(canonical.tailSeconds);
  const [fade, setFade] = useState(canonical.fadeOutSeconds);
  const [range, setRange] = useState<RenderRange['kind']>('entireArrangement');
  const [trackId, setTrackId] = useState('');
  const [stems, setStems] = useState(false);
  const [outputPath, setOutputPath] = useState('');
  const [saving, setSaving] = useState(false);
  const [job, setJob] = useState<BackgroundJobStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  useEffect(() => {
    setEnabled(canonical.mastering !== null);
    if (canonical.mastering) setTarget(canonical.mastering);
    setTail(canonical.tailSeconds);
    setFade(canonical.fadeOutSeconds);
  }, [canonical]);
  const active = Boolean(job && ['queued', 'running', 'cancelling'].includes(job.state));
  const busy = saving || Boolean(active);
  const jobId = job?.id;
  const chooseOutput = async () => {
    try {
      const path = stems ? await chooseRenderStemDirectory() : await saveRenderWav('riffra-render');
      if (path) setOutputPath(path);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    }
  };
  useEffect(() => {
    if (!active || !jobId) return;
    let cancelled = false;
    let timer: number | undefined;
    const poll = async () => {
      try {
        const current = await props.api.getBackgroundJob(jobId);
        if (cancelled) return;
        if (!current) throw new Error('Render job is unavailable');
        setJob(current);
        if (['queued', 'running', 'cancelling'].includes(current.state))
          timer = window.setTimeout(() => void poll(), 200);
      } catch (failure) {
        if (!cancelled) {
          setError(failure instanceof Error ? failure.message : String(failure));
          setJob(null);
        }
      }
    };
    timer = window.setTimeout(() => void poll(), 200);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [active, jobId, props.api]);
  const save = async (render: boolean) => {
    if (busy) return;
    setSaving(true);
    setError(null);
    try {
      const settings: MixdownSettings = {
        ...canonical,
        tailSeconds: tail,
        fadeOutSeconds: fade,
        mastering: enabled ? target : null,
      };
      if (!sameMixdownSettings(settings, canonical)) {
        const result = await props.api.setMixdown(settings);
        if (!alive.current || !props.applyCanonicalState(result.canonical)) return;
        if (result.projection.state === 'failed') throw new Error(result.projection.message);
      }
      if (render) {
        const renderRange: RenderRange =
          range === 'loopRange' ? { kind: range } : { kind: 'entireArrangement' };
        const queued = stems
          ? await props.api.startStemRenderJob({
              allTracks: !trackId,
              trackIds: trackId ? [trackId] : [],
              range: renderRange,
              outputDirectory: outputPath || null,
            })
          : await props.api.startRenderJob({
              range: renderRange,
              trackId: trackId || null,
              normalize: false,
              outputPath: outputPath || null,
            });
        if (alive.current) setJob(queued);
      }
    } catch (failure) {
      if (alive.current) setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      if (alive.current) setSaving(false);
    }
  };
  const cancel = async () => {
    if (!jobId) return;
    try {
      const result = await props.api.cancelBackgroundJob(jobId);
      if (alive.current) setJob(result);
    } catch (failure) {
      if (alive.current) setError(failure instanceof Error ? failure.message : String(failure));
    }
  };
  const result =
    job?.state === 'completed' && (job.kind === 'render' || job.kind === 'renderStems')
      ? job.result
      : null;
  return (
    <div className={styles.routingOverlay} role="presentation">
      <section
        className={styles.routingDialog}
        role="dialog"
        aria-modal="true"
        aria-label="Mixdown"
        onKeyDown={(e) => {
          if (e.key === 'Escape' && !busy) props.onClose();
        }}
      >
        <header>
          <strong>MIXDOWN</strong>
          <button autoFocus type="button" disabled={busy} onClick={props.onClose}>
            Close
          </button>
        </header>
        <p>
          Master FX and Gain process the production mix. Loudness Mastering processes the rendered
          WAV.
        </p>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void save((e.nativeEvent as SubmitEvent).submitter?.getAttribute('value') === 'render');
          }}
        >
          <fieldset disabled={busy} className={styles.mixdownFields}>
            <label>
              <input
                type="checkbox"
                checked={enabled}
                onChange={(e) => setEnabled(e.currentTarget.checked)}
              />
              Loudness Mastering
            </label>
            {enabled && (
              <>
                <label>
                  Target Integrated LUFS{' '}
                  <input
                    type="number"
                    min={-70}
                    max={-5}
                    step={0.1}
                    required
                    value={Number.isFinite(target.integratedLufs) ? target.integratedLufs : ''}
                    onChange={(e) =>
                      setTarget({ ...target, integratedLufs: e.currentTarget.valueAsNumber })
                    }
                  />
                </label>
                <label>
                  True Peak ceiling (dBTP){' '}
                  <input
                    type="number"
                    min={-9}
                    max={0}
                    step={0.1}
                    required
                    value={Number.isFinite(target.truePeakDb) ? target.truePeakDb : ''}
                    onChange={(e) =>
                      setTarget({ ...target, truePeakDb: e.currentTarget.valueAsNumber })
                    }
                  />
                </label>
                <label>
                  Loudness Range ceiling, monitor only (LU){' '}
                  <input
                    type="number"
                    min={1}
                    max={50}
                    step={0.1}
                    required
                    value={Number.isFinite(target.loudnessRangeLu) ? target.loudnessRangeLu : ''}
                    onChange={(e) =>
                      setTarget({ ...target, loudnessRangeLu: e.currentTarget.valueAsNumber })
                    }
                  />
                </label>
              </>
            )}
            <label>
              Tail (seconds)
              <input
                type="number"
                min={0}
                step={0.1}
                required
                value={Number.isFinite(tail) ? tail : ''}
                onChange={(e) => setTail(e.currentTarget.valueAsNumber)}
              />
            </label>
            <label>
              Fade out (seconds)
              <input
                type="number"
                min={0}
                step={0.1}
                required
                value={Number.isFinite(fade) ? fade : ''}
                onChange={(e) => setFade(e.currentTarget.valueAsNumber)}
              />
            </label>
            <button type="submit" value="save">
              Save settings
            </button>
          </fieldset>
          <fieldset disabled={busy} className={styles.mixdownFields}>
            <label>
              Range
              <select
                value={range}
                onChange={(e) => setRange(e.currentTarget.value as RenderRange['kind'])}
              >
                <option value="entireArrangement">Entire arrangement</option>
                <option value="loopRange">Loop range</option>
              </select>
            </label>
            <label>
              Source
              <select value={trackId} onChange={(e) => setTrackId(e.currentTarget.value)}>
                <option value="">All Tracks</option>
                {props.session.arrangement.tracks.map((track) => (
                  <option key={track.id} value={track.id}>
                    {track.name}
                  </option>
                ))}
              </select>
            </label>
            <label>
              <input
                type="checkbox"
                checked={stems}
                onChange={(e) => {
                  setStems(e.currentTarget.checked);
                  setOutputPath('');
                }}
              />
              Aligned Track stems
            </label>
            <div className={styles.renderOutputPicker}>
              <span>{stems ? 'Stem output directory' : 'WAV output file'}</span>
              <button type="button" onClick={() => void chooseOutput()}>
                Choose output
              </button>
              <small>{outputPath || 'Keep output in the Riffra render store only'}</small>
            </div>
            {stems && (
              <p>
                Stems retain Group, Return and Master FX without individual loudness mastering.
                Nonlinear shared FX can make their sum differ from the complete mix.
              </p>
            )}
            <button type="submit" value="render">
              Save and render
            </button>
          </fieldset>
        </form>
        {active && (
          <button
            type="button"
            onClick={() => void cancel()}
            disabled={job?.state === 'cancelling'}
          >
            Cancel render
          </button>
        )}
        {job && (
          <p role="status">
            {job.message}
            {job.progress !== null ? ` · ${Math.round(job.progress * 100)}%` : ''}
          </p>
        )}
        {job?.state === 'failed' && <p role="alert">{job.message}</p>}
        {result &&
          ('stems' in result ? (
            result.stems.map((stem) => (
              <section key={stem.trackId}>
                <h3>{stem.trackName}</h3>
                <p>{stem.path}</p>
                <Measurements value={stem.measurements} warnings={stem.warnings} />
              </section>
            ))
          ) : (
            <section>
              <p>{result.path}</p>
              <Measurements value={result.measurements} warnings={result.warnings} />
              {result.mastering && (
                <p>
                  Target {result.mastering.target.integratedLufs} LUFS · Completed{' '}
                  {(
                    result.mastering.target.integratedLufs +
                    result.mastering.deviation.integratedLufs
                  ).toFixed(2)}{' '}
                  LUFS
                  {result.mastering.truePeakCorrectionDb > 0 && (
                    <>
                      {' '}
                      · True Peak ceiling limited the gain by{' '}
                      {result.mastering.truePeakCorrectionDb.toFixed(2)} dB
                    </>
                  )}
                </p>
              )}
            </section>
          ))}
        <RenderHistoryPanel
          api={props.api}
          refreshToken={job?.state === 'completed' ? job.id : ''}
        />
        {error && <p role="alert">{error}</p>}
      </section>
    </div>
  );
}
