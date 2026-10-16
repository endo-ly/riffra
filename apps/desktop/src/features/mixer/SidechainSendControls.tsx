import type { ReactNode } from 'react';
import type { ArrangementMutationResult, SidechainSend } from '@/model/domain';
import type { ArrangeWorkspaceApi } from '@/features/arrange/arrange-api';
import { LevelSlider } from './LevelSlider';

interface SidechainSendControlsProps {
  send: SidechainSend;
  title: ReactNode;
  api: ArrangeWorkspaceApi;
  commit: (operation: Promise<ArrangementMutationResult>) => void;
  onOpenAutomation: () => void;
}

export function SidechainSendControls({
  send,
  title,
  api,
  commit,
  onOpenAutomation,
}: SidechainSendControlsProps) {
  return (
    <fieldset>
      <strong>{title}</strong>
      <button type="button" onClick={onOpenAutomation}>
        Automation
      </button>
      <LevelSlider
        ariaLabel={`Sidechain ${send.id} level`}
        initial={send.levelDb}
        enabled={send.enabled}
        preview={(value) => api.previewSidechainSendLevel(send.id, value)}
        submit={(value) => api.updateSidechainSend(send.id, { levelDb: value })}
        commit={commit}
      />
      <label>
        Position{' '}
        <select
          aria-label={`Sidechain ${send.id} position`}
          value={send.position}
          onChange={(e) =>
            commit(
              api.updateSidechainSend(send.id, {
                position: e.currentTarget.value as SidechainSend['position'],
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
            commit(api.updateSidechainSend(send.id, { enabled: e.currentTarget.checked }))
          }
        />
        Enabled
      </label>
      <button type="button" onClick={() => commit(api.removeSidechainSend(send.id))}>
        Remove Sidechain
      </button>
    </fieldset>
  );
}
