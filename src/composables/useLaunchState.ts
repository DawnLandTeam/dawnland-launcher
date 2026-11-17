import { ref } from 'vue';
import { listen } from '@tauri-apps/api/event';
import { useInstances } from './useInstances';
import { createLogger } from '../utils/logger';

const logger = createLogger('useLaunchState');

// Global singleton states for instances
export const launchingInstances = ref<Set<string>>(new Set());
export const jvmSpawnedInstances = ref<Set<string>>(new Set());
export const runningInstances = ref<Set<string>>(new Set());
export const repairingInstances = ref<Set<string>>(new Set());

export interface InstanceStatePayload {
  versionId: string;
  status: 'running' | 'exited' | 'repairing' | 'repairing_complete';
  exitCode?: number;
  missingCount?: number;
  isOpenJ9?: boolean;
  crashReport?: string;
  intentionallyKilled?: boolean;
}

export function isInstanceActive(instanceId: string): boolean {
  if (!instanceId) return false;
  const idLower = instanceId.toLowerCase();
  for (const id of runningInstances.value) {
    if (id.toLowerCase() === idLower) return true;
  }
  for (const id of launchingInstances.value) {
    if (id.toLowerCase() === idLower) return true;
  }
  for (const id of jvmSpawnedInstances.value) {
    if (id.toLowerCase() === idLower) return true;
  }
  return false;
}

let isInitialized = false;

export async function initGlobalLaunchState() {
  if (isInitialized) return;
  isInitialized = true;

  // Sync initially running instances from backend
  try {
    const { fetchInstances } = useInstances();
    const instances = await fetchInstances();
    for (const inst of instances) {
      if (inst.status === 'running') {
        runningInstances.value.add(inst.id);
        jvmSpawnedInstances.value.add(inst.id);
      }
    }
  } catch (e) {
    logger.warn('Failed to sync initial running instances:', e);
  }

  // Listen for instance state changes globally across all views
  await listen<InstanceStatePayload>('instance-state-changed', (event) => {
    const { versionId, status } = event.payload;
    if (status === 'running') {
      jvmSpawnedInstances.value.add(versionId);
      if (!launchingInstances.value.has(versionId)) {
        runningInstances.value.add(versionId);
      } else {
        setTimeout(() => {
          if (launchingInstances.value.has(versionId)) {
            runningInstances.value.add(versionId);
            launchingInstances.value.delete(versionId);
          }
        }, 8000);
      }
    } else if (status === 'exited') {
      jvmSpawnedInstances.value.delete(versionId);
      runningInstances.value.delete(versionId);
      launchingInstances.value.delete(versionId);
      repairingInstances.value.delete(versionId);
    } else if (status === 'repairing') {
      repairingInstances.value.add(versionId);
    } else if (status === 'repairing_complete') {
      repairingInstances.value.delete(versionId);
    }
  });

  const markLaunchedIfEarlyOutput = (line: string) => {
    if (
      line.includes('Backend library:') ||
      line.includes('LWJGL') ||
      line.includes('Setting user') ||
      line.includes('OpenAL initialized')
    ) {
      for (const id of launchingInstances.value) {
        if (jvmSpawnedInstances.value.has(id)) {
          runningInstances.value.add(id);
          launchingInstances.value.delete(id);
        }
      }
    }
  };

  // Listen for game-log and game-logs-batch in case LWJGL / Backend library comes up early
  await listen<{ line?: string }>('game-log', (event) => {
    markLaunchedIfEarlyOutput(event.payload.line || '');
  });

  await listen<Array<{ line?: string }>>('game-logs-batch', (event) => {
    for (const item of event.payload) {
      markLaunchedIfEarlyOutput(item.line || '');
    }
  });

  // Keep running state in sync whenever instances change
  await listen('instances-changed', async () => {
    try {
      const { fetchInstances } = useInstances();
      const instances = await fetchInstances();
      const activeIds = new Set(instances.filter(i => i.status === 'running').map(i => i.id.toLowerCase()));
      for (const id of Array.from(runningInstances.value)) {
        if (!activeIds.has(id.toLowerCase()) && !jvmSpawnedInstances.value.has(id)) {
          runningInstances.value.delete(id);
        }
      }
    } catch {
      // Ignore background refresh errors
    }
  });
}
