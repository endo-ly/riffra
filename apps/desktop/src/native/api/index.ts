import { isNativeRuntime } from '@/native/invoke';
import { createBrowserPreviewApi } from '@/native/preview/preview-api';
import { createNativeApi } from './native-implementation';

export const defaultNativeApi = isNativeRuntime() ? createNativeApi() : createBrowserPreviewApi();
