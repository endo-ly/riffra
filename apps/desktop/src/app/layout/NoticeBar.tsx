import type { useAppUpdater } from '@/app/runtime/useAppUpdater';
import { BackgroundJobNotice } from '@/app/notices/BackgroundJobNotice';
import { BrowserPreviewNotice } from '@/app/notices/BrowserPreviewNotice';
import { PlaybackNotice } from '@/app/notices/PlaybackNotice';
import { SafeModeNotice } from '@/app/notices/SafeModeNotice';
import { UpdateNotice } from '@/app/notices/UpdateNotice';
import { MissingDependenciesNotice } from '@/features/project/MissingDependenciesNotice';
import { RecoveryNotice } from '@/features/project/RecoveryNotice';
import { ScoreSyncNotice } from '@/features/project/ScoreSyncNotice';
import styles from './NoticeBar.module.css';

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
      <BrowserPreviewNotice />
    </div>
  );
}
