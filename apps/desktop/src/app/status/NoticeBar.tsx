import { BackgroundJobNotice } from './BackgroundJobNotice';
import { PreviewModeNotice } from './PreviewModeNotice';
import { PlaybackNotice } from './PlaybackNotice';
import { SafeModeNotice } from './SafeModeNotice';
import { UpdateNotice } from './UpdateNotice';
import { MissingDependenciesNotice } from '@/features/project/MissingDependenciesNotice';
import { RecoveryNotice } from '@/features/project/RecoveryNotice';
import { ScoreSyncNotice } from '@/features/project/ScoreSyncNotice';
import styles from './NoticeBar.module.css';

/**
 * App-wide conditions that continue alongside editing, docked below the
 * Global Control Bar. The order here is the display order: danger, then warning, then info.
 */
export function NoticeBar() {
  return (
    <div className={styles.noticeBar}>
      <ScoreSyncNotice />
      <PlaybackNotice />
      <MissingDependenciesNotice />
      <SafeModeNotice />
      <RecoveryNotice />
      <BackgroundJobNotice />
      <UpdateNotice />
      <PreviewModeNotice />
    </div>
  );
}
