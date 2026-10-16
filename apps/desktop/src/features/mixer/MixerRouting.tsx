import type {
  Arrangement,
  ArrangementMutationResult,
  AutomationTarget,
  Send,
  Track,
} from '@/model/domain';
import type { ArrangeWorkspaceApi } from '@/features/arrange/arrange-api';
import { sidechainTargetName } from '@/features/arrange/model/automation-targets';
import { LevelSlider } from './LevelSlider';
import { SidechainSendControls } from './SidechainSendControls';
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
  return (
    <LevelSlider
      ariaLabel={`Send ${send.id} level`}
      initial={send.levelDb}
      enabled={send.enabled}
      preview={(value) => api.previewSendLevel(send.id, value)}
      submit={(value) => api.updateSend(send.id, { levelDb: value })}
      commit={commit}
    />
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
  const sidechainSends = arrangement.mixer.sidechainSends.filter(
    (send) => send.sourceTrackId === track.id,
  );
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
        <h3>Sidechain</h3>
        {sidechainSends.map((send) => (
          <SidechainSendControls
            key={send.id}
            send={send}
            title={sidechainTargetName(arrangement, send.targetDeviceId) ?? 'Unknown device'}
            api={api}
            commit={commit}
            onOpenAutomation={() =>
              onOpenAutomation({ kind: 'sidechainSendLevel', sidechainSendId: send.id })
            }
          />
        ))}
        {!sidechainSends.length && <p>Add sidechain sends from the target device in Devices.</p>}
      </section>
    </div>
  );
}
