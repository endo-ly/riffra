import type {
  ArrangementMutationResult,
  EffectOwner,
  MixBusKind,
  MixBusPatch,
  SendPatch,
  SendPosition,
  SidechainSendPatch,
} from '@/model/domain';
import { dispatchControl } from '../invoke';

export interface MixerApi {
  addMixBus(name: string, kind: MixBusKind): Promise<ArrangementMutationResult>;
  updateMixBus(busId: string, patch: MixBusPatch): Promise<ArrangementMutationResult>;
  removeMixBus(busId: string): Promise<ArrangementMutationResult>;
  setTrackOutput(trackId: string, busId: string | null): Promise<ArrangementMutationResult>;
  addSend(
    trackId: string,
    returnBusId: string,
    levelDb: number,
    position: SendPosition,
  ): Promise<ArrangementMutationResult>;
  updateSend(sendId: string, patch: SendPatch): Promise<ArrangementMutationResult>;
  removeSend(sendId: string): Promise<ArrangementMutationResult>;
  addSidechainSend(
    sourceTrackId: string,
    targetDeviceId: string,
    levelDb: number,
    position: SendPosition,
  ): Promise<ArrangementMutationResult>;
  updateSidechainSend(
    sidechainSendId: string,
    patch: SidechainSendPatch,
  ): Promise<ArrangementMutationResult>;
  removeSidechainSend(sidechainSendId: string): Promise<ArrangementMutationResult>;
  setMasterGainDb(gainDb: number): Promise<ArrangementMutationResult>;
  previewMasterGainDb(gainDb: number): Promise<void>;
  previewChannelMix(owner: EffectOwner, patch: { gainDb?: number; pan?: number }): Promise<void>;
  previewSendLevel(sendId: string, levelDb: number): Promise<void>;
  previewSidechainSendLevel(sidechainSendId: string, levelDb: number): Promise<void>;
}

function addMixBus(name: string, kind: MixBusKind) {
  return dispatchControl({ command: 'mixer.bus.add', params: { name, kind } });
}

function updateMixBus(busId: string, patch: MixBusPatch) {
  return dispatchControl({ command: 'mixer.bus.update', params: { busId, patch } });
}

function removeMixBus(busId: string) {
  return dispatchControl({ command: 'mixer.bus.remove', params: { busId } });
}

function setTrackOutput(trackId: string, busId: string | null) {
  return dispatchControl({ command: 'mixer.output.set', params: { trackId, busId } });
}

function addSend(trackId: string, returnBusId: string, levelDb: number, position: SendPosition) {
  return dispatchControl({
    command: 'mixer.send.add',
    params: { trackId, returnBusId, levelDb, position },
  });
}

function updateSend(sendId: string, patch: SendPatch) {
  return dispatchControl({ command: 'mixer.send.update', params: { sendId, patch } });
}

function removeSend(sendId: string) {
  return dispatchControl({ command: 'mixer.send.remove', params: { sendId } });
}

function addSidechainSend(
  sourceTrackId: string,
  targetDeviceId: string,
  levelDb: number,
  position: SendPosition,
) {
  return dispatchControl({
    command: 'mixer.sidechain.add',
    params: { sourceTrackId, targetDeviceId, levelDb, position },
  });
}

function updateSidechainSend(sidechainSendId: string, patch: SidechainSendPatch) {
  return dispatchControl({
    command: 'mixer.sidechain.update',
    params: { sidechainSendId, patch },
  });
}

function removeSidechainSend(sidechainSendId: string) {
  return dispatchControl({
    command: 'mixer.sidechain.remove',
    params: { sidechainSendId },
  });
}

async function setMasterGainDb(gainDb: number): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'audio.master-gain.set', params: { gainDb } });
}

async function previewMasterGainDb(gainDb: number): Promise<void> {
  await dispatchControl({ command: 'audio.master-gain.preview', params: { gainDb } });
}

async function previewChannelMix(
  owner: EffectOwner,
  patch: { gainDb?: number; pan?: number },
): Promise<void> {
  await dispatchControl({
    command: 'mixer.channel.preview',
    params: { owner, gainDb: patch.gainDb, pan: patch.pan },
  });
}

async function previewSendLevel(sendId: string, levelDb: number): Promise<void> {
  await dispatchControl({ command: 'mixer.send.preview', params: { sendId, levelDb } });
}

async function previewSidechainSendLevel(sidechainSendId: string, levelDb: number): Promise<void> {
  await dispatchControl({
    command: 'mixer.sidechain.preview',
    params: { sidechainSendId, levelDb },
  });
}

export const mixerApi: MixerApi = {
  addMixBus,
  updateMixBus,
  removeMixBus,
  setTrackOutput,
  addSend,
  updateSend,
  removeSend,
  addSidechainSend,
  updateSidechainSend,
  removeSidechainSend,
  setMasterGainDb,
  previewMasterGainDb,
  previewChannelMix,
  previewSendLevel,
  previewSidechainSendLevel,
};
