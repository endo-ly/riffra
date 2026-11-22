import type { MixdownSettings } from '@/model/generated';

export function sameMixdownSettings(left: MixdownSettings, right: MixdownSettings) {
  const sameMastering =
    left.mastering === null || right.mastering === null
      ? left.mastering === right.mastering
      : left.mastering.integratedLufs === right.mastering.integratedLufs &&
        left.mastering.truePeakDb === right.mastering.truePeakDb &&
        left.mastering.loudnessRangeLu === right.mastering.loudnessRangeLu;
  return (
    left.musicalEndTick === right.musicalEndTick &&
    left.tailSeconds === right.tailSeconds &&
    left.fadeOutSeconds === right.fadeOutSeconds &&
    left.pluginWarmUpSeconds === right.pluginWarmUpSeconds &&
    left.renderMode === right.renderMode &&
    left.sampleRate === right.sampleRate &&
    left.blockSize === right.blockSize &&
    sameMastering
  );
}
