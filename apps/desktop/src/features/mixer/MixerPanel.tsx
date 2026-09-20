import type { AudioStatus, CanonicalState, CreativeSession } from '@/model/domain';
import type { ArrangeWorkspaceApi } from '@/features/arrange/arrange-api';
import { MixerMasterChannelStrip } from './MixerMasterChannelStrip';
import { MixerTrackChannelStrip } from './MixerTrackChannelStrip';
import styles from './Mixer.module.css';

interface MixerPanelProps {
  session: CreativeSession;
  selectedTrackId: string | null;
  api: ArrangeWorkspaceApi;
  applyCanonicalState: (canonical: CanonicalState) => boolean;
  setAudio: (audio: AudioStatus) => void;
  onSelectTrack: (trackId: string) => void;
  onError?: (message: string) => void;
  disabled?: boolean;
}

export function MixerPanel(props: MixerPanelProps) {
  const { arrangement } = props.session;
  return (
    <div className={styles.panel} aria-label="Mixer" data-mixer-panel>
      <header className={styles.panelHeader}>
        <div>
          <strong>MIXER</strong>
          <span>Track balance and stereo output</span>
        </div>
        <small>{arrangement.tracks.length} TRACKS</small>
      </header>

      <div className={styles.trackViewport}>
        {arrangement.tracks.length ? (
          <div className={styles.trackRail}>
            {arrangement.tracks.map((track, trackIndex) => (
              <MixerTrackChannelStrip
                key={track.id}
                sessionId={props.session.sessionId}
                track={track}
                trackIndex={trackIndex}
                volumeAutomation={arrangement.automationLanes.find(
                  (lane) => lane.trackId === track.id && lane.parameter === 'volume',
                )}
                panAutomation={arrangement.automationLanes.find(
                  (lane) => lane.trackId === track.id && lane.parameter === 'pan',
                )}
                selected={props.selectedTrackId === track.id}
                api={props.api}
                applyCanonicalState={props.applyCanonicalState}
                onSelect={() => props.onSelectTrack(track.id)}
                onError={props.onError}
                disabled={props.disabled}
              />
            ))}
          </div>
        ) : (
          <div className={styles.empty}>
            <strong>No tracks</strong>
            <span>Add tracks from the Timeline to start mixing.</span>
          </div>
        )}
      </div>

      <MixerMasterChannelStrip
        session={props.session}
        api={props.api}
        applyCanonicalState={props.applyCanonicalState}
        setAudio={props.setAudio}
        disabled={props.disabled}
      />
    </div>
  );
}
