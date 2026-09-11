import type { RuntimeProjectionStatus } from '@/model/domain';
import {
  getHostGeneration,
  getProjectEpoch,
  HostConnectionChangedError,
  invokeHost,
  ProjectChangedError,
} from '../invoke';

interface PendingSeek {
  kind: 'seek';
  tick: number;
  waiters: { resolve: () => void; reject: (error: unknown) => void }[];
}

type TransportBarrier = 'play_timeline' | 'stop_timeline' | 'go_to_start_timeline';

interface PendingBarrier {
  kind: 'barrier';
  command: TransportBarrier;
  resolve: () => void;
  reject: (error: unknown) => void;
}

type PendingCommand = PendingSeek | PendingBarrier;

type CommandOutcome = { ok: true } | { ok: false; error: unknown };

interface TransportScheduler {
  key: string;
  hostGeneration: number;
  projectEpoch: number;
  executing: boolean;
  commands: PendingCommand[];
}

const transportSchedulers = new Map<string, TransportScheduler>();

function currentTransportScheduler(): TransportScheduler {
  const hostGeneration = getHostGeneration();
  const projectEpoch = getProjectEpoch();
  const key = `host:${hostGeneration}:project:${projectEpoch}`;
  let scheduler = transportSchedulers.get(key);
  if (!scheduler) {
    scheduler = {
      key,
      hostGeneration,
      projectEpoch,
      executing: false,
      commands: [],
    };
    transportSchedulers.set(key, scheduler);
  }
  return scheduler;
}

function invokeTransportCommand(
  scheduler: TransportScheduler,
  command: PendingCommand,
): Promise<void> {
  if (scheduler.hostGeneration !== getHostGeneration()) {
    throw new HostConnectionChangedError();
  }
  if (scheduler.projectEpoch !== getProjectEpoch()) {
    throw new ProjectChangedError();
  }
  return command.kind === 'seek'
    ? invokeHost<void>('seek_timeline', { tick: command.tick })
    : invokeHost<void>(command.command);
}

function settleCommand(command: PendingCommand, outcome: CommandOutcome): void {
  if (command.kind === 'seek') {
    command.waiters.forEach(({ resolve, reject }) => {
      if (outcome.ok) resolve();
      else reject(outcome.error);
    });
    return;
  }
  if (outcome.ok) command.resolve();
  else command.reject(outcome.error);
}

function pumpTransportScheduler(scheduler: TransportScheduler): void {
  if (scheduler.executing) return;
  const command = scheduler.commands.shift();
  if (command === undefined) {
    if (transportSchedulers.get(scheduler.key) === scheduler) {
      transportSchedulers.delete(scheduler.key);
    }
    return;
  }

  scheduler.executing = true;
  const operation = Promise.resolve().then(() => invokeTransportCommand(scheduler, command));
  void operation
    .then(
      () => settleCommand(command, { ok: true }),
      (error: unknown) => settleCommand(command, { ok: false, error }),
    )
    .finally(() => {
      scheduler.executing = false;
      pumpTransportScheduler(scheduler);
    })
    .catch(() => undefined);
}

function queueSeek(tick: number): Promise<void> {
  const scheduler = currentTransportScheduler();
  return new Promise<void>((resolve, reject) => {
    const last = scheduler.commands.at(-1);
    if (last?.kind === 'seek') {
      last.tick = tick;
      last.waiters.push({ resolve, reject });
    } else {
      scheduler.commands.push({ kind: 'seek', tick, waiters: [{ resolve, reject }] });
    }
    pumpTransportScheduler(scheduler);
  });
}

function queueTransportCommand(command: TransportBarrier): Promise<void> {
  const scheduler = currentTransportScheduler();
  return new Promise<void>((resolve, reject) => {
    scheduler.commands.push({ kind: 'barrier', command, resolve, reject });
    pumpTransportScheduler(scheduler);
  });
}

export async function getRuntimeProjectionStatus(): Promise<RuntimeProjectionStatus> {
  return await invokeHost<RuntimeProjectionStatus>('get_runtime_projection_status');
}

export async function retryRuntimeProjection(): Promise<RuntimeProjectionStatus> {
  return await invokeHost<RuntimeProjectionStatus>('retry_runtime_projection');
}

export async function playTimeline(): Promise<void> {
  await queueTransportCommand('play_timeline');
}

export async function stopTimeline(): Promise<void> {
  await queueTransportCommand('stop_timeline');
}

export async function goToStartTimeline(): Promise<void> {
  await queueTransportCommand('go_to_start_timeline');
}

export async function seekTimeline(tick: number): Promise<void> {
  await queueSeek(tick);
}
