import { isNativeRuntime } from '../invoke';
import { createBrowserPreviewApi } from './browser-preview';
import { createNativeApi } from './native-implementation';

export const defaultNativeApi = isNativeRuntime() ? createNativeApi() : createBrowserPreviewApi();
