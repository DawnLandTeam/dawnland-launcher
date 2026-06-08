import { describe, it, expect, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';

// Mock Tauri IPC globally for this test file
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn((cmd: string) => {
    if (cmd === 'get_system_info') {
      return Promise.resolve({ os: 'windows', arch: 'x86_64' });
    }
    return Promise.resolve(null);
  })
}));

describe('Tauri IPC Mock Test', () => {
  it('should successfully mock get_system_info command', async () => {
    const result = await invoke('get_system_info');
    
    expect(invoke).toHaveBeenCalledWith('get_system_info');
    expect(result).toEqual({ os: 'windows', arch: 'x86_64' });
  });
});
