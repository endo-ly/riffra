export type LocalHostInfo = {
  instanceId: string;
  pid: number;
  dataRoot: string;
  startedAtMs: number;
  projectName: string | null;
  safeMode: boolean;
  status: string;
};
