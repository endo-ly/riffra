import type {
  BackgroundJobStatus,
  CreativeSession,
  LoudnessMastering,
  MixdownSettings,
  RenderRange,
} from '@/model/domain';
import { chooseRenderStemDirectory, saveRenderWav } from '@/native/dialog';
import { errorText } from '@/shared/errors';
import { useSessionContext } from '@/shared/session/session-context';
import controls from '@/shared/ui/controls.module.css';
import { Icon } from '@/shared/ui/primitives';
import { useEffect, useId, useRef, useState } from 'react';
import styles from './Mixdown.module.css';
import { DialogRow, DialogSection, MixerDialog } from './MixerDialog';
import { RenderHistoryPanel } from './RenderHistoryPanel';
import { RenderMeasurements } from './RenderMeasurements';

interface Props {
  session: CreativeSession;
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
    left.pluginWarmUpSeconds === right.pluginWarmUpSeconds &&
    left.sampleRate === right.sampleRate &&
    left.blockSize === right.blockSize &&
    sameMastering
  );
}

interface NumberFieldProps {
  label: string;
  /** Full accessible name; the visible label relies on its section for context. */
  name: string;
  unit: string;
  value: number | null;
  min?: number;
  max?: number;
  required?: boolean;
  onChange: (value: number | null) => void;
}

/** A labelled number in a well, its unit trailing the value. */
function NumberField({ required = true, ...props }: NumberFieldProps) {
  return (
    <label className={styles.numberField}>
      <span>{props.label}</span>
      <span className={styles.numberWell}>
        <input
          type="number"
          aria-label={props.name}
          min={props.min}
          max={props.max}
          step={0.1}
          required={required}
          value={props.value !== null && Number.isFinite(props.value) ? props.value : ''}
          onChange={(e) =>
            props.onChange(e.currentTarget.value === '' ? null : e.currentTarget.valueAsNumber)
          }
        />
        <small>{props.unit}</small>
      </span>
    </label>
  );
}

const RANGES: readonly [RenderRange['kind'], string][] = [
  ['entireArrangement', 'Entire arrangement'],
  ['loopRange', 'Loop range'],
];

const OUTPUTS: readonly [boolean, string][] = [
  [false, 'Stereo mix'],
  [true, 'Aligned Track stems'],
];

function fileName(path: string) {
  return path.split(/[\\/]/).at(-1) || path;
}

export function MixdownDialog(props: Props) {
  const { api, commit: sessionCommit } = useSessionContext();
  const canonical = props.session.settings.mixdown;
  const [enabled, setEnabled] = useState(canonical.mastering !== null);
  const [target, setTarget] = useState<LoudnessMastering>(
    canonical.mastering ?? { integratedLufs: -14, truePeakDb: -1, loudnessRangeLu: 11 },
  );
  const [tail, setTail] = useState(canonical.tailSeconds);
  const [fade, setFade] = useState(canonical.fadeOutSeconds);
  const [warmUp, setWarmUp] = useState(canonical.pluginWarmUpSeconds);
  const [range, setRange] = useState<RenderRange['kind']>('entireArrangement');
  const [trackId, setTrackId] = useState('');
  const [stems, setStems] = useState(false);
  const [outputPath, setOutputPath] = useState('');
  const [saving, setSaving] = useState(false);
  const [job, setJob] = useState<BackgroundJobStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const formId = useId();
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
    setWarmUp(canonical.pluginWarmUpSeconds);
  }, [canonical]);
  const active = Boolean(job && ['queued', 'running', 'cancelling'].includes(job.state));
  const busy = saving || Boolean(active);
  const jobId = job?.id;
  const chooseOutput = async () => {
    try {
      const path = stems ? await chooseRenderStemDirectory() : await saveRenderWav('riffra-render');
      if (path) setOutputPath(path);
    } catch (failure) {
      setError(errorText(failure));
    }
  };
  useEffect(() => {
    if (!active || !jobId) return;
    let cancelled = false;
    let timer: number | undefined;
    const poll = async () => {
      try {
        const current = await api.getBackgroundJob(jobId);
        if (cancelled) return;
        if (!current) throw new Error('Render job is unavailable');
        setJob(current);
        if (['queued', 'running', 'cancelling'].includes(current.state))
          timer = window.setTimeout(() => void poll(), 200);
      } catch (failure) {
        if (!cancelled) {
          setError(errorText(failure));
          setJob(null);
        }
      }
    };
    timer = window.setTimeout(() => void poll(), 200);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [active, jobId, api]);
  const save = async (render: boolean) => {
    if (busy) return;
    setSaving(true);
    setError(null);
    try {
      const settings: MixdownSettings = {
        ...canonical,
        tailSeconds: tail,
        fadeOutSeconds: fade,
        pluginWarmUpSeconds: warmUp,
        mastering: enabled ? target : null,
      };
      if (!sameMixdownSettings(settings, canonical)) {
        const result = await sessionCommit(api.setMixdown(settings));
        if (!alive.current || !result || result.projection.state === 'failed') return;
      }
      if (render) {
        const renderRange: RenderRange =
          range === 'loopRange' ? { kind: range } : { kind: 'entireArrangement' };
        const queued = stems
          ? await api.startStemRenderJob({
              allTracks: !trackId,
              trackIds: trackId ? [trackId] : [],
              range: renderRange,
              outputDirectory: outputPath || null,
            })
          : await api.startRenderJob({
              range: renderRange,
              trackId: trackId || null,
              normalize: false,
              outputPath: outputPath || null,
            });
        if (alive.current) setJob(queued);
      }
    } catch (failure) {
      if (alive.current) setError(errorText(failure));
    } finally {
      if (alive.current) setSaving(false);
    }
  };
  const cancel = async () => {
    if (!jobId) return;
    try {
      const result = await api.cancelBackgroundJob(jobId);
      if (alive.current) setJob(result);
    } catch (failure) {
      if (alive.current) setError(errorText(failure));
    }
  };
  const result =
    job?.state === 'completed' && (job.kind === 'render' || job.kind === 'renderStems')
      ? job.result
      : null;
  const progress = job?.progress ?? null;
  return (
    <MixerDialog
      label="Mixdown"
      title="Mixdown"
      subtitle="Render to WAV"
      icon="wave"
      closeDisabled={busy}
      onClose={props.onClose}
      footer={
        <>
          <div className={styles.status}>
            {job && job.state !== 'failed' && (
              <>
                <span className={styles.statusLine} role="status">
                  <span>{job.message}</span>
                  {progress !== null && <output>{Math.round(progress * 100)}%</output>}
                </span>
                {active && (
                  <span className={styles.progress}>
                    <i style={{ width: `${(progress ?? 0) * 100}%` }} />
                  </span>
                )}
              </>
            )}
            {job?.state === 'failed' && (
              <p className={styles.error} role="alert">
                {job.message}
              </p>
            )}
            {error && (
              <p className={styles.error} role="alert">
                {error}
              </p>
            )}
          </div>
          {active ? (
            <button
              type="button"
              className={controls.smallButton}
              disabled={job?.state === 'cancelling'}
              onClick={() => void cancel()}
            >
              Cancel render
            </button>
          ) : (
            <button
              type="submit"
              form={formId}
              value="save"
              className={controls.smallButton}
              disabled={busy}
            >
              Save settings
            </button>
          )}
          <button
            type="submit"
            form={formId}
            value="render"
            className={controls.primaryButton}
            disabled={busy}
          >
            Save and render
          </button>
        </>
      }
    >
      <form
        id={formId}
        onSubmit={(e) => {
          e.preventDefault();
          void save((e.nativeEvent as SubmitEvent).submitter?.getAttribute('value') === 'render');
        }}
      >
        <fieldset disabled={busy} className={styles.fieldset}>
          <DialogSection title="Render">
            <DialogRow label="Range">
              <div className={controls.segmented} role="group" aria-label="Range">
                {RANGES.map(([kind, label]) => (
                  <button
                    key={kind}
                    type="button"
                    aria-pressed={range === kind}
                    onClick={() => setRange(kind)}
                  >
                    {label}
                  </button>
                ))}
              </div>
            </DialogRow>
            <DialogRow label="Source">
              <select
                className={controls.control}
                aria-label="Source"
                value={trackId}
                onChange={(e) => setTrackId(e.currentTarget.value)}
              >
                <option value="">All Tracks</option>
                {props.session.arrangement.tracks.map((track) => (
                  <option key={track.id} value={track.id}>
                    {track.name}
                  </option>
                ))}
              </select>
            </DialogRow>
            <DialogRow label="Output">
              <div className={controls.segmented} role="group" aria-label="Output">
                {OUTPUTS.map(([value, label]) => (
                  <button
                    key={label}
                    type="button"
                    aria-pressed={stems === value}
                    onClick={() => {
                      if (stems === value) return;
                      setStems(value);
                      setOutputPath('');
                    }}
                  >
                    {label}
                  </button>
                ))}
              </div>
            </DialogRow>
            <DialogRow label={stems ? 'Stem folder' : 'WAV file'}>
              <div className={styles.destination}>
                <span className={styles.path} title={outputPath || undefined}>
                  {outputPath ? (
                    <code>{outputPath}</code>
                  ) : (
                    <em>Keep output in the Riffra render store only</em>
                  )}
                </span>
                {outputPath && (
                  <button
                    type="button"
                    className={controls.iconButton}
                    aria-label="Clear output"
                    title="Keep output in the Riffra render store only"
                    onClick={() => setOutputPath('')}
                  >
                    <Icon name="close" />
                  </button>
                )}
                <button
                  type="button"
                  className={controls.smallButton}
                  onClick={() => void chooseOutput()}
                >
                  <Icon name="folder" />
                  Choose output
                </button>
              </div>
            </DialogRow>
            {stems && (
              <p className={controls.hint}>
                Stems retain Group, Return and Master FX without individual loudness mastering.
                Nonlinear shared FX can make their sum differ from the complete mix.
              </p>
            )}
          </DialogSection>
          <DialogSection
            title="Loudness Mastering"
            action={
              <input
                type="checkbox"
                role="switch"
                className={controls.switch}
                aria-label="Loudness Mastering"
                checked={enabled}
                onChange={(e) => setEnabled(e.currentTarget.checked)}
              />
            }
          >
            <p className={controls.hint}>
              Master FX and Gain process the production mix. Loudness Mastering processes the
              rendered WAV.
            </p>
            <fieldset disabled={!enabled} className={styles.numbers}>
              <NumberField
                label="Integrated target"
                name="Target Integrated LUFS"
                unit="LUFS"
                min={-70}
                max={-5}
                value={target.integratedLufs}
                onChange={(value) => setTarget({ ...target, integratedLufs: value ?? NaN })}
              />
              <NumberField
                label="True Peak ceiling"
                name="True Peak ceiling (dBTP)"
                unit="dBTP"
                min={-9}
                max={0}
                value={target.truePeakDb}
                onChange={(value) => setTarget({ ...target, truePeakDb: value ?? NaN })}
              />
              <NumberField
                label="Range ceiling · monitor only"
                name="Loudness Range ceiling, monitor only (LU)"
                unit="LU"
                min={0}
                required={false}
                value={target.loudnessRangeLu}
                onChange={(value) => setTarget({ ...target, loudnessRangeLu: value })}
              />
            </fieldset>
          </DialogSection>
          <DialogSection title="Ending">
            <div className={styles.numbers}>
              <NumberField
                label="Tail"
                name="Tail (seconds)"
                unit="s"
                min={0}
                value={tail}
                onChange={(value) => setTail(value ?? NaN)}
              />
              <NumberField
                label="Fade out"
                name="Fade out (seconds)"
                unit="s"
                min={0}
                value={fade}
                onChange={(value) => setFade(value ?? NaN)}
              />
            </div>
          </DialogSection>
          <DialogSection title="Plugin preparation">
            <div className={styles.numbers}>
              <NumberField
                label="Preparation time"
                name="Plugin preparation time (seconds)"
                unit="s"
                min={0}
                max={60}
                value={warmUp}
                onChange={(value) => setWarmUp(value ?? NaN)}
              />
            </div>
          </DialogSection>
        </fieldset>
      </form>
      {result && (
        <DialogSection title="Last render">
          {'stems' in result ? (
            result.stems.map((stem) => (
              <article key={stem.trackId} className={styles.result}>
                <header>
                  <strong>{stem.trackName}</strong>
                  <small title={stem.path}>{stem.path}</small>
                </header>
                <RenderMeasurements value={stem.measurements} warnings={stem.warnings} />
              </article>
            ))
          ) : (
            <article className={styles.result}>
              <header>
                <strong>{fileName(result.path)}</strong>
                <small title={result.path}>{result.path}</small>
              </header>
              <RenderMeasurements value={result.measurements} warnings={result.warnings} />
              {result.mastering && (
                <p className={controls.hint}>
                  Target {result.mastering.target.integratedLufs} LUFS · Completed{' '}
                  {(
                    result.mastering.target.integratedLufs +
                    result.mastering.deviation.integratedLufs
                  ).toFixed(2)}{' '}
                  LUFS
                  {result.mastering.truePeakCorrectionDb > 0 && (
                    <>
                      {' '}
                      · True Peak output gain correction{' '}
                      {result.mastering.truePeakCorrectionDb.toFixed(2)} dB
                    </>
                  )}
                </p>
              )}
            </article>
          )}
        </DialogSection>
      )}
      <RenderHistoryPanel refreshToken={job?.state === 'completed' ? job.id : ''} />
    </MixerDialog>
  );
}
