import { test as base, expect } from '@playwright/test';

export type MockedIpcCall = {
  cmd: string;
  args: any;
};

type TauriFixtures = {
  mockTauri: {
    setMockResponses: (responses: Record<string, any>) => Promise<void>;
    getIpcCalls: () => Promise<MockedIpcCall[]>;
    clearIpcCalls: () => Promise<void>;
  };
};

// Extend base test with our mock fixture
export const test = base.extend<TauriFixtures>({
  mockTauri: async ({ page }, use) => {
    let ipcCalls: MockedIpcCall[] = [];
    let mockResponses: Record<string, any> = {
      // Default common mocks
      'get_system_info': { os: 'windows', arch: 'x86_64' },
      'get_accounts': [],
      'scan_installed_instances': [],
      'get_servers': [],
      'scan_local_javas': [],
      'get_system_locale': 'en-US',
      'get_system_memory': { totalMb: 16384, recommendedMaxMb: 4096 },
      'get_settings': { java_path: '', memory_limit: 4096, language: 'en' },
      'plugin:app|version': '0.0.1',
      'plugin:window|show': null,
    };

    // Expose functions to the browser context
    await page.exposeFunction('__recordIpcCall', (cmd: string, args: any) => {
      ipcCalls.push({ cmd, args });
    });

    await page.exposeFunction('__getIpcResponse', (cmd: string) => {
      if (cmd in mockResponses) {
        return mockResponses[cmd];
      }
      return null;
    });

    // Inject the Tauri mock early before page loads
    await page.addInitScript(() => {
      // @ts-ignore
      window.isTauri = true;
      // @ts-ignore
      window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
      // @ts-ignore
      window.__TAURI_INTERNALS__.metadata = {
        currentWindow: { label: 'main' },
        currentWebview: { windowLabel: 'main', label: 'main' }
      };
      // @ts-ignore
      window.__TAURI_INTERNALS__.invoke = async (cmd: string, args: any) => {
        // @ts-ignore
        await window.__recordIpcCall(cmd, args);
        // @ts-ignore
        const response = await window.__getIpcResponse(cmd);
        
        // Emulate typical Tauri error handling where some commands return Result<T, E>
        // Here we just return the raw response, but if we need to mock an error,
        // we can add a convention like { __error: "message" }
        if (response && typeof response === 'object' && response.__error) {
          throw new Error(response.__error);
        }
        
        return response;
      };
    });

    // Provide the fixture to the test
    await use({
      setMockResponses: async (responses: Record<string, any>) => {
        mockResponses = { ...mockResponses, ...responses };
      },
      getIpcCalls: async () => ipcCalls,
      clearIpcCalls: async () => {
        ipcCalls = [];
      }
    });
  }
});

export { expect };
