import { onMounted, onUnmounted } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

/** Payload of the `instance-icon-updated` event emitted by the Rust backend. */
export interface InstanceIconUpdatedPayload {
  instanceId: string;
  iconPath: string;
}

export const INSTANCE_ICON_UPDATED_EVENT = 'instance-icon-updated';

/**
 * Subscribe to backend icon cache updates for the lifetime of the calling component.
 */
export function useInstanceIconUpdates(onUpdate: (payload: InstanceIconUpdatedPayload) => void) {
  let unlisten: UnlistenFn | null = null;
  let disposed = false;

  onMounted(async () => {
    const fn = await listen<InstanceIconUpdatedPayload>(INSTANCE_ICON_UPDATED_EVENT, (event) => {
      onUpdate(event.payload);
    });
    if (disposed) {
      fn();
    } else {
      unlisten = fn;
    }
  });

  onUnmounted(() => {
    disposed = true;
    unlisten?.();
    unlisten = null;
  });
}
