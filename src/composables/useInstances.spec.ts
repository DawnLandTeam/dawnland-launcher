import { describe, it, expect, vi, beforeEach } from 'vitest';
import { useInstances } from './useInstances';
import { invoke } from '@tauri-apps/api/core';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn()
}));

describe('useInstances composable', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // Reset global state if possible, though instances is module-scoped
    const { instances, isLoaded } = useInstances();
    instances.value = [];
    isLoaded.value = false;
  });

  it('fetches instances and sets isLoaded to true', async () => {
    const mockData = [{ id: '1', name: 'Instance 1' }];
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
    instances.value = [{ id: '1', name: 'Cached' }];
    
    vi.mocked(invoke).mockResolvedValueOnce([{ id: '2', name: 'New' }]);

    // Call without force
    const result1 = await fetchInstances();
    
    // It returns cached immediately and does a background fetch
    expect(result1).toEqual([{ id: '1', name: 'Cached' }]);
    expect(invoke).toHaveBeenCalledTimes(1);
    
    // Wait for the background promise to resolve (macro task tick)
    await new Promise(resolve => setTimeout(resolve, 0));
    expect(instances.value).toEqual([{ id: '2', name: 'New' }]);
    
    // Call with force
    vi.mocked(invoke).mockResolvedValueOnce([{ id: '3', name: 'Forced' }]);
    const result2 = await fetchInstances(true);
    
    expect(invoke).toHaveBeenCalledTimes(2);
    expect(result2).toEqual([{ id: '3', name: 'Forced' }]);
  });
});
