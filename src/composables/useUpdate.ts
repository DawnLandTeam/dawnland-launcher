import { ref, shallowRef } from 'vue';

export interface CustomUpdate {
  version: string;
  body: string;
  md5?: string;
  url?: string;
}

// Global state for update
export const hasUpdateAvailable = ref(false);
export const globalUpdateInfo = shallowRef<CustomUpdate | null>(null);

export function setUpdateAvailable(update: CustomUpdate | null) {
  if (update) {
    hasUpdateAvailable.value = true;
    globalUpdateInfo.value = update;
  } else {
    hasUpdateAvailable.value = false;
    globalUpdateInfo.value = null;
  }
}

export function parseUpdateData(data: any, targetOS: string): CustomUpdate | null {
  if (!data || !data.version) return null;

  const platformData = data.platforms?.[targetOS];
  if (!platformData) {
    console.warn(`No update asset found for target OS: ${targetOS}`);
    return null;
  }

  return {
    version: data.version,
    body: data.notes || '',
    md5: platformData.md5,
    url: platformData.url,
  };
}
