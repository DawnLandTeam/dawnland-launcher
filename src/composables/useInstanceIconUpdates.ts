import { onMounted, onUnmounted, ref } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

/** Payload of the `instance-icon-updated` event emitted by the Rust backend. */
export interface InstanceIconUpdatedPayload {
  instanceId: string;
  iconPath: string;
  updatedAt?: number;
}

export interface InstanceWithIcon {
  id: string;
  iconPath?: string | null;
}

export const INSTANCE_ICON_UPDATED_EVENT = 'instance-icon-updated';

/**
 * Latest icon path known in this session per instance ID (survives stale scan results
 * that started before an `instance-icon-updated` event arrived).
 */
const latestIconPaths = new Map<string, string>();
/**
 * Reactive cache-busting revision per icon file path so replacing `icon.png` in place
 * with a new image forces mounted `<img>` elements to reload.
 */
const iconPathRevisions = ref<Record<string, number>>({});

/** Record a newly cached icon path and bump its cache-busting revision. */
export function recordInstanceIconUpdate(payload: InstanceIconUpdatedPayload): void {
  const rev = payload.updatedAt ?? Date.now();
  latestIconPaths.set(payload.instanceId, payload.iconPath);
  iconPathRevisions.value = {
    ...iconPathRevisions.value,
    [payload.iconPath]: rev,
  };
}

/** Return the current cache-busting revision for `iconPath`, if it was updated in-session. */
export function getIconPathRevision(iconPath: string | null | undefined): number | undefined {
  if (!iconPath) return undefined;
  return iconPathRevisions.value[iconPath];
}

/**
 * Merge any in-session icon updates into a freshly scanned instance list so a scan that
 * started before an `instance-icon-updated` event cannot clobber the patched `iconPath`.
 */
export function applyCachedInstanceIcons<T extends InstanceWithIcon>(items: T[]): T[] {
  for (const item of items) {
    if (item.iconPath) {
      latestIconPaths.set(item.id, item.iconPath);
    } else {
      const cached = latestIconPaths.get(item.id);
      if (cached) {
        item.iconPath = cached;
      }
    }
  }
  return items;
}

/**
 * Subscribe to backend icon cache updates for the lifetime of the calling component.
 */
export function useInstanceIconUpdates(onUpdate: (payload: InstanceIconUpdatedPayload) => void) {
  let unlisten: UnlistenFn | null = null;
  let disposed = false;

  onMounted(async () => {
    const fn = await listen<InstanceIconUpdatedPayload>(INSTANCE_ICON_UPDATED_EVENT, (event) => {
      recordInstanceIconUpdate(event.payload);
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
