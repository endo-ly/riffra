import type { CreativeSession } from '@/model/generated';
import { MASTER_GAIN_DB } from '@/model/mix-range';
import { useAudioMeters } from '@/shared/audio/audio-meters';
import { Meter } from '@/shared/ui/primitives';
import { ValueSlider } from '@/shared/ui/ValueSlider';
import { useMasterGainValue } from './hooks/useMasterGainValue';
import styles from './AudioMonitor.module.css';

interface AudioMonitorProps {
  session: CreativeSession;
  disabled?: boolean;
}

export function AudioMonitor(props: AudioMonitorProps) {
  const { session } = props;
  const meters = useAudioMeters();
  const master = useMasterGainValue(session, { disabled: props.disabled });

  return (
    <div className={styles.monitor} data-audio-monitor aria-label="Audio monitor">
      <div className={styles.levelMeter} aria-label="Input and output levels">
        <span>In</span>
        <Meter value={meters.inputPeak * 100} />
        <span>Out</span>
        <Meter value={meters.outputPeak * 100} />
      </div>
      <label className={styles.master}>
        <span>Master</span>
        <strong>{master.value.toFixed(1)} dB</strong>
        <ValueSlider
          control={master}
          range={MASTER_GAIN_DB}
          disabled={props.disabled}
          ariaLabel="Master volume"
        />
      </label>
    </div>
  );
}
