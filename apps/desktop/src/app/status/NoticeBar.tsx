import type { useAppUpdater } from '@/app/hooks/useAppUpdater';
import { BackgroundJobNotice } from '@/app/status/BackgroundJobNotice';
import { PreviewModeNotice } from '@/app/status/PreviewModeNotice';
import { PlaybackNotice } from '@/app/status/PlaybackNotice';
import { SafeModeNotice } from '@/app/status/SafeModeNotice';
import { UpdateNotice } from '@/app/status/UpdateNotice';
import { MissingDependenciesNotice } from '@/features/project/MissingDependenciesNotice';
import { RecoveryNotice } from '@/features/project/RecoveryNotice';
import { ScoreSyncNotice } from '@/features/project/ScoreSyncNotice';
import styles from '@/app/status/NoticeBar.module.css';

/**
 * App-wide conditions that continue alongside editing, docked below the
 * Global Control Bar. The order here is the display order: danger, then warning, then info.
 */
export function NoticeBar({ appUpdater }: { appUpdater: ReturnType<typeof useAppUpdater> }) {
  return (
    <div className={styles.noticeBar}>
      <ScoreSyncNotice />
      <PlaybackNotice />
      <MissingDependenciesNotice />
      <SafeModeNotice />
      <RecoveryNotice />
      <BackgroundJobNotice />
      <UpdateNotice appUpdater={appUpdater} />
      <PreviewModeNotice />
    </div>
  );
}
