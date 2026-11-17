import { ref } from 'vue';
import { listen } from '@tauri-apps/api/event';

export interface GameLogPayload {
  versionId?: string;
  type: string;
  line: string;
}

const MAX_GAME_LOG_LINES = 2000;

const gameLogs = ref<string[]>([]);
const instanceLogs = ref<Map<string, string[]>>(new Map());
let activeInstanceId = ref<string>('');
let isInitialized = false;

function handleSingleLog(
  payload: GameLogPayload,
  onLine?: (payload: GameLogPayload) => void,
): void {
  const vId = payload.versionId || activeInstanceId.value || 'default';
  let list = instanceLogs.value.get(vId);
  if (!list) {
    list = [];
    instanceLogs.value.set(vId, list);
  }
  list.push(payload.line);
  if (list.length > MAX_GAME_LOG_LINES) {
    list.splice(0, list.length - MAX_GAME_LOG_LINES);
  }

  if (!payload.versionId || payload.versionId === activeInstanceId.value) {
    gameLogs.value.push(payload.line);
    if (gameLogs.value.length > MAX_GAME_LOG_LINES) {
      gameLogs.value = gameLogs.value.slice(-MAX_GAME_LOG_LINES);
    }
  }

  if (onLine) {
    onLine(payload);
  }
}

export function useGameLogStore() {
  async function init(onLine?: (payload: GameLogPayload) => void): Promise<void> {
    if (isInitialized) return;
    isInitialized = true;

    await listen<GameLogPayload>('game-log', (event) => {
      handleSingleLog(event.payload, onLine);
    });

    await listen<GameLogPayload[]>('game-logs-batch', (event) => {
      for (const item of event.payload) {
        handleSingleLog(item, onLine);
      }
    });
  }

  function setActiveInstanceId(instanceId: string): void {
    activeInstanceId.value = instanceId;
  }

  function clearActiveLogs(): void {
    gameLogs.value = [];
  }

  function renameInstanceLogs(oldId: string, newId: string): void {
    if (instanceLogs.value.has(oldId)) {
      const logs = instanceLogs.value.get(oldId);
      if (logs) {
        instanceLogs.value.set(newId, logs);
      }
      instanceLogs.value.delete(oldId);
    }
  }

  function getInstanceLogs(instanceId: string): string[] {
    return instanceLogs.value.get(instanceId) || gameLogs.value;
  }

  function __resetForTest(): void {
    gameLogs.value = [];
    instanceLogs.value = new Map();
    activeInstanceId.value = '';
    isInitialized = false;
  }

  return {
    gameLogs,
    instanceLogs,
    init,
    setActiveInstanceId,
    clearActiveLogs,
    renameInstanceLogs,
    getInstanceLogs,
    __resetForTest,
  };
}
