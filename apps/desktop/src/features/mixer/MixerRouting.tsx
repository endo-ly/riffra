import { useEffect, useRef, useState } from 'react';
import type {
  Arrangement,
  ArrangementMutationResult,
  AutomationTarget,
  Send,
  Track,
} from '@/model/domain';
import type { ArrangeWorkspaceApi } from '@/features/arrange/arrange-api';
import styles from './Mixer.module.css';

interface Props {
  arrangement: Arrangement;
  track: Track;
  api: ArrangeWorkspaceApi;
  commit: (operation: Promise<ArrangementMutationResult>) => void;
  onClose: () => void;
  onOpenAutomation: (target: AutomationTarget) => void;
}

function SendLevel({
  send,
  api,
  commit,
}: {
  send: Send;
  api: ArrangeWorkspaceApi;
  commit: Props['commit'];
}) {
  const [draft, setDraft] = useState(send.levelDb);
  const timer = useRef<number | undefined>(undefined);
  const chain = useRef(Promise.resolve());
  const version = useRef(0);
  const submitted = useRef(send.levelDb);
  useEffect(() => {
    setDraft(send.levelDb);
    submitted.current = send.levelDb;
  }, [send.levelDb]);
  useEffect(
    () => () => {
      version.current++;
      window.clearTimeout(timer.current);
    },
    [],
  );
  useEffect(() => {
    if (send.enabled) return;
    version.current++;
    window.clearTimeout(timer.current);
  }, [send.enabled]);
  const preview = (value: number) => {
    if (!send.enabled) return;
    window.clearTimeout(timer.current);
    const request = ++version.current;
    timer.current = window.setTimeout(() => {
      chain.current = chain.current
        .catch(() => undefined)
        .then(async () => {
          if (request === version.current) await api.previewSendLevel(send.id, value);
        })
        .catch(() => undefined);
    }, 40);
  };
  const finish = (value: number) => {
    window.clearTimeout(timer.current);
    if (submitted.current === value) return;
    const request = ++version.current;
    submitted.current = value;
    void chain.current
      .catch(() => undefined)
      .then(async () => {
        if (request !== version.current) return;
        if (send.enabled) await api.previewSendLevel(send.id, value).catch(() => undefined);
        if (request !== version.current) return;
        commit(
          api.updateSend(send.id, { levelDb: value }).catch((error) => {
            if (request === version.current) {
              setDraft(send.levelDb);
              submitted.current = send.levelDb;
              preview(send.levelDb);
            }
            throw error;
          }),
        );
      });
  };
  return (
    <label>
      Level{' '}
      <input
        type="range"
        aria-label={`Send ${send.id} level`}
        min={-90}
        max={24}
        step={0.1}
        value={draft}
        onChange={(e) => {
          const value = Number(e.currentTarget.value);
          setDraft(value);
          preview(value);
        }}
        onPointerUp={(e) => finish(Number(e.currentTarget.value))}
        onKeyUp={(e) => {
          if (e.key !== 'Escape') finish(Number(e.currentTarget.value));
        }}
        onPointerCancel={() => {
          submitted.current = send.levelDb;
          setDraft(send.levelDb);
          preview(send.levelDb);
        }}
        onKeyDown={(e) => {
          if (e.key === 'Escape') {
            e.stopPropagation();
            submitted.current = send.levelDb;
            setDraft(send.levelDb);
            preview(send.levelDb);
          }
        }}
        onBlur={(e) => finish(Number(e.currentTarget.value))}
      />
      <output>{draft.toFixed(1)} dB</output>
    </label>
  );
}

export function MixerRouting({
  arrangement,
  track,
  api,
  commit,
  onClose,
  onOpenAutomation,
}: Props) {
  const returns = arrangement.mixer.buses.filter((bus) => bus.kind === 'return');
  const sends = arrangement.mixer.sends.filter((send) => send.trackId === track.id);
  return (
    <div className={styles.routingOverlay} role="presentation" onMouseDown={onClose}>
      <section
        className={styles.routingDialog}
        role="dialog"
        aria-modal="true"
        aria-label={`${track.name} routing`}
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === 'Escape') onClose();
        }}
      >
        <header>
          <strong>{track.name} · Routing</strong>
          <button autoFocus type="button" onClick={onClose}>
            Close
          </button>
        </header>
        <label>
          Main output{' '}
          <select
            aria-label="Main output"
            value={track.outputBusId ?? ''}
            onChange={(e) => commit(api.setTrackOutput(track.id, e.currentTarget.value || null))}
          >
            <option value="">Master</option>
            {arrangement.mixer.buses
              .filter((bus) => bus.kind === 'group')
              .map((bus) => (
                <option key={bus.id} value={bus.id}>
                  {bus.name}
                </option>
              ))}
          </select>
        </label>
        <h3>Sends</h3>
        {sends.map((send) => (
          <fieldset key={send.id}>
            <label>
              Return{' '}
              <select
                aria-label={`Send ${send.id} return`}
                value={send.returnBusId}
                onChange={(e) =>
                  commit(api.updateSend(send.id, { returnBusId: e.currentTarget.value }))
                }
              >
                {returns
                  .filter(
                    (bus) =>
                      bus.id === send.returnBusId ||
                      !sends.some((other) => other.returnBusId === bus.id),
                  )
                  .map((bus) => (
                    <option key={bus.id} value={bus.id}>
                      {bus.name}
                    </option>
                  ))}
              </select>
            </label>
            <button
              type="button"
              onClick={() => onOpenAutomation({ kind: 'sendLevel', sendId: send.id })}
            >
              Automation
            </button>
            <SendLevel send={send} api={api} commit={commit} />
            <label>
              Position{' '}
              <select
                aria-label={`Send ${send.id} position`}
                value={send.position}
                onChange={(e) =>
                  commit(
                    api.updateSend(send.id, {
                      position: e.currentTarget.value as Send['position'],
                    }),
                  )
                }
              >
                <option value="postFader">Post-Fader</option>
                <option value="preFader">Pre-Fader</option>
              </select>
            </label>
            <label>
              <input
                type="checkbox"
                checked={send.enabled}
                onChange={(e) =>
                  commit(api.updateSend(send.id, { enabled: e.currentTarget.checked }))
                }
              />
              Enabled
            </label>
            <button type="button" onClick={() => commit(api.removeSend(send.id))}>
              Remove Send
            </button>
          </fieldset>
        ))}
        <label>
          Add Send{' '}
          <select
            aria-label="Add Send"
            value=""
            onChange={(e) => {
              if (e.currentTarget.value)
                commit(api.addSend(track.id, e.currentTarget.value, -6, 'postFader'));
            }}
          >
            <option value="">Choose Return</option>
            {returns
              .filter((bus) => !sends.some((send) => send.returnBusId === bus.id))
              .map((bus) => (
                <option key={bus.id} value={bus.id}>
                  {bus.name}
                </option>
              ))}
          </select>
        </label>
        {!returns.length && <p>Add a Return channel to create Sends.</p>}
      </section>
    </div>
  );
}
