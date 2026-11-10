import { describe, it, expect, vi, beforeEach } from 'vitest';
import { useInstances, _resetStateForTesting } from './useInstances';
import { invoke } from '@tauri-apps/api/core';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn()
}));

describe('useInstances composable', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    _resetStateForTesting();
  });

  it('fetches instances and sets isLoaded to true', async () => {
    const mockData = [{ id: '1', name: 'Instance 1', mcVersion: '1.20.1', loaderType: 'Vanilla', status: 'ready' as const }];
    vi.mocked(invoke).mockResolvedValueOnce(mockData);

    const { fetchInstances, instances, isLoaded } = useInstances();
    
    expect(isLoaded.value).toBe(false);
    
    const result = await fetchInstances();
    
    expect(invoke).toHaveBeenCalledWith('scan_installed_instances');
    expect(result).toEqual(mockData);
    expect(instances.value).toEqual(mockData);
    expect(isLoaded.value).toBe(true);
  });

  it('does not refetch if already loaded unless force is true', async () => {
    const { fetchInstances, instances, isLoaded } = useInstances();
    
    // Setup initial state
    isLoaded.value = true;
    instances.value = [{ id: '1', name: 'Cached', mcVersion: '1.20.1', loaderType: 'Vanilla', status: 'ready' as const }];
    
    vi.mocked(invoke).mockResolvedValueOnce([{ id: '2', name: 'New', mcVersion: '1.20.1', loaderType: 'Vanilla', status: 'ready' as const }]);

    // Call without force
    const result1 = await fetchInstances();
    
    // It returns cached immediately and does a background fetch
    expect(result1).toEqual([{ id: '1', name: 'Cached', mcVersion: '1.20.1', loaderType: 'Vanilla', status: 'ready' as const }]);
    expect(invoke).toHaveBeenCalledTimes(1);
    
    // Wait for the background promise to resolve (macro task tick)
    await new Promise(resolve => setTimeout(resolve, 0));
    expect(instances.value).toEqual([{ id: '2', name: 'New', mcVersion: '1.20.1', loaderType: 'Vanilla', status: 'ready' as const }]);
    
    // Call with force
    vi.mocked(invoke).mockResolvedValueOnce([{ id: '3', name: 'Forced', mcVersion: '1.20.1', loaderType: 'Vanilla', status: 'ready' as const }]);
    const result2 = await fetchInstances(true);
    
    expect(invoke).toHaveBeenCalledTimes(2);
    expect(result2).toEqual([{ id: '3', name: 'Forced', mcVersion: '1.20.1', loaderType: 'Vanilla', status: 'ready' as const }]);
  });

  it('queues a fresh scan if force fetch is called while a scan is in-flight', async () => {
    let resolveFirstScan: (val: any) => void = () => {};
    const firstScanPromise = new Promise((resolve) => {
      resolveFirstScan = resolve;
    });

    vi.mocked(invoke)
      .mockImplementationOnce(() => firstScanPromise as any)
      .mockResolvedValueOnce([{ id: '2', name: 'Second Scan', mcVersion: '1.20.1', loaderType: 'Vanilla', status: 'ready' as const }]);

    const { fetchInstances, instances } = useInstances();

    // Start first scan in-flight
    const p1 = fetchInstances(true);
    // While first scan is in-flight, a force scan arrives (e.g. task completed)
    const p2 = fetchInstances(true);

    // Resolve first scan with old data
    resolveFirstScan([{ id: '1', name: 'First Scan', mcVersion: '1.20.1', loaderType: 'Vanilla', status: 'ready' as const }]);

    await p1;
    const res2 = await p2;

    expect(invoke).toHaveBeenCalledTimes(2);
    expect(res2).toEqual([{ id: '2', name: 'Second Scan', mcVersion: '1.20.1', loaderType: 'Vanilla', status: 'ready' as const }]);
    expect(instances.value).toEqual([{ id: '2', name: 'Second Scan', mcVersion: '1.20.1', loaderType: 'Vanilla', status: 'ready' as const }]);
  });
});
