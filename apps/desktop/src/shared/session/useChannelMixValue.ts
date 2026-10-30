import type { CanonicalState, MixBus, Track } from '@/model/generated';
import { useSessionContext } from '@/shared/session/session-context';
import { useCallback } from 'react';
import { usePreviewedValue } from './usePreviewedValue';

type MixParameter = 'gainDb' | 'pan';

/** Shares the preview and commit lifecycle for a track or mix bus parameter. */
export function useChannelMixValue(
  channel: Track | MixBus,
  parameter: MixParameter,
  options: { sessionId: string; disabled?: boolean; onError?: (message: string) => void },
) {
  const { api } = useSessionContext();
  const isBus = channel.kind === 'group' || channel.kind === 'return';
  const read = useCallback(
    (canonical: CanonicalState) => {
      const channels = isBus
        ? canonical.session.arrangement.mixer.buses
        : canonical.session.arrangement.tracks;
      return channels.find((candidate) => candidate.id === channel.id)?.[parameter];
    },
    [channel.id, isBus, parameter],
  );
  const submit = useCallback(
    (value: number) => {
      const patch = { [parameter]: value } as { gainDb?: number; pan?: number };
      return isBus ? api.updateMixBus(channel.id, patch) : api.updateTrack(channel.id, patch);
    },
    [api, channel.id, isBus, parameter],
  );
  const preview = useCallback(
    (value: number) =>
      api.previewChannelMix(
        { kind: isBus ? 'bus' : 'track', id: channel.id },
        { [parameter]: value },
      ),
    [api, channel.id, isBus, parameter],
  );

  return usePreviewedValue({
    value: channel[parameter],
    sessionId: options.sessionId,
    target: `${isBus ? 'bus' : 'track'}:${channel.id}:${parameter}`,
    read,
    submit,
    preview,
    disabled: options.disabled,
    onError: options.onError,
  });
}
