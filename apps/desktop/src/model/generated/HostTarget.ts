export type HostTarget =
  | { type: 'embedded' }
  | { type: 'registration'; instanceId: string }
  | { type: 'dataRoot'; dataRoot: string };
