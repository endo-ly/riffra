import type { PluginEntry, PluginStatus, RackDevice } from '@/lib/domain';
import { pluginParameterValuesForSession } from '@/lib/plugin-session';

/**
 * Pure projections of the rack portion of a Session. Each function takes
 * the current rack and returns a new rack, so the session-mutation logic that
 * was previously buried inside App's async handlers can be unit-tested without
 * a native runtime.
 */

/** Replaces any existing plugin device with a freshly loaded one. */
export function rackWithPluginLoaded(
  rack: RackDevice[],
  plugin: PluginEntry,
  runtime: PluginStatus | null | undefined,
  options: { parameterValues: number[]; bypassed: boolean; stateData: string | null },
): RackDevice[] {
  return [
    ...rack.filter((device) => device.kind !== 'plugin'),
    {
      id: `plugin:${plugin.id}`,
      name: plugin.name,
      kind: 'plugin',
      path: plugin.path,
      bypassed: options.bypassed,
      gainDb: 0,
      parameterValues: pluginParameterValuesForSession(
        runtime?.parameters,
        options.parameterValues,
      ),
      stateData: runtime?.stateData ?? options.stateData,
    },
  ];
}

/** Removes the plugin device while leaving every other device untouched. */
export function rackWithoutPlugin(rack: RackDevice[]): RackDevice[] {
  return rack.filter((device) => device.kind !== 'plugin');
}

/** Updates the bypassed flag on the plugin device only. */
export function rackWithPluginBypassed(rack: RackDevice[], bypassed: boolean): RackDevice[] {
  return rack.map((device) => (device.kind === 'plugin' ? { ...device, bypassed } : device));
}

/** Updates the captured parameter values and state blob on the plugin device. */
export function rackWithPluginParameter(
  rack: RackDevice[],
  parameterValues: number[],
  stateData: string | null,
): RackDevice[] {
  return rack.map((device) =>
    device.kind === 'plugin' ? { ...device, parameterValues, stateData } : device,
  );
}
