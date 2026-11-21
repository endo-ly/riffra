import type { AudioClip, CanonicalState } from '@/model/generated';
import { useSessionContext } from '@/session/session-context';
import { usePreviewedValue } from '@/session/hooks/usePreviewedValue';

/** Preview and commit lifecycle of an Audio Clip's gain or pan. */
export function useClipMixValue(
  clip: AudioClip,
  sessionId: string,
  parameter: 'gainDb' | 'pan',
  onError: (message: string) => void,
) {
  const { api } = useSessionContext();
  const read = (canonical: CanonicalState) =>
    canonical.session.arrangement.audioClips.find((item) => item.id === clip.id)?.[parameter];
  return usePreviewedValue({
    value: clip[parameter],
    sessionId,
    target: `audioClip:${clip.id}:${parameter}`,
    read,
    submit: (value) => api.updateAudioClip(clip.id, { [parameter]: value }),
    onError,
  });
}
