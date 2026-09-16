
import { useStorage } from '@vueuse/core';
export const isMinimalMode = useStorage('launcher-minimal-mode', false);
export function toggleAppMode() { isMinimalMode.value = !isMinimalMode.value; }
