import { describe, it, expect, vi, beforeEach } from 'vitest';
import { listen } from '@tauri-apps/api/event';
import { useGameLogStore, type GameLogPayload } from './useGameLogStore';

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}));

describe('useGameLogStore composable', () => {
  const listeners = new Map<string, (event: { payload: unknown }) => void>();

  beforeEach(() => {
    vi.clearAllMocks();
    listeners.clear();
    const store = useGameLogStore();
    store.__resetForTest();

    vi.mocked(listen).mockImplementation(async (eventName, handler) => {
      listeners.set(eventName, handler as (event: { payload: unknown }) => void);
      return () => {
        listeners.delete(eventName);
      };
    });
  });

  it('handles both single game-log and batched game-logs-batch events per instance', async () => {
    const store = useGameLogStore();
    const observedLines: string[] = [];
    store.setActiveInstanceId('inst-a');

    await store.init((payload) => {
      observedLines.push(payload.line);
    });

    const singleHandler = listeners.get('game-log');
    const batchHandler = listeners.get('game-logs-batch');
    expect(singleHandler).toBeDefined();
    expect(batchHandler).toBeDefined();

    singleHandler?.({
      payload: {
        versionId: 'inst-a',
        type: 'stdout',
        line: 'Backend library: LWJGL version 3.3.3',
      } satisfies GameLogPayload,
    });

    batchHandler?.({
      payload: [
        { versionId: 'inst-a', type: 'stdout', line: 'Setting user: Player1' },
        { versionId: 'inst-b', type: 'stderr', line: 'Other instance warning' },
      ] satisfies GameLogPayload[],
    });

    expect(observedLines).toEqual([
      'Backend library: LWJGL version 3.3.3',
      'Setting user: Player1',
      'Other instance warning',
    ]);
    expect(store.gameLogs.value).toEqual([
      'Backend library: LWJGL version 3.3.3',
      'Setting user: Player1',
    ]);
    expect(store.getInstanceLogs('inst-b')).toEqual(['Other instance warning']);

    store.renameInstanceLogs('inst-b', 'inst-b-renamed');
    expect(store.getInstanceLogs('inst-b-renamed')).toEqual(['Other instance warning']);

    store.clearActiveLogs();
    expect(store.gameLogs.value).toEqual([]);
  });
});
