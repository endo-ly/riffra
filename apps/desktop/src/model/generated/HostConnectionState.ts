export type HostConnectionMode = 'embedded' | 'attached' | 'disconnected';

export type HostConnectionState = {
  mode: HostConnectionMode;
  generation: number;
  dataRoot: string | null;
  instanceId: string | null;
  pid: number | null;
  reason: string | null;
};
