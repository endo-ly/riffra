export const MAX_PERSISTED_PLUGIN_PARAMETERS = 512;

export function pluginParameterValuesForSession(
  parameters: Array<{ value: number }> | undefined,
  fallback: number[] = [],
): number[] {
  return (parameters?.map((parameter) => parameter.value) ?? fallback)
    .slice(0, MAX_PERSISTED_PLUGIN_PARAMETERS);
}

export function shouldRestoreIndividualParameters(stateData: string | null): boolean {
  return !stateData;
}
