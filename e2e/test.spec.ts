import { test, expect } from '@playwright/test';

test.describe('Dawnland Launcher UI', () => {
  test.beforeEach(async ({ page }) => {
    // Inject Tauri mock APIs before the page loads
    await page.addInitScript(() => {
      window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
      window.__TAURI_INTERNALS__.invoke = async (cmd: string, args: any) => {
        console.log(`[Tauri Mock] IPC invoked: ${cmd}`, args);
        
        // Mock specific commands
        if (cmd === 'get_system_info') {
          return { os: 'windows', arch: 'x86_64' };
        }
        if (cmd === 'scan_installed_instances') {
          return [];
        }
        if (cmd === 'get_vanilla_versions') {
          return { versions: [] };
        }
        
        return null; // Default fallback for other commands
      };
    });
  });

  test('should open and render the main window', async ({ page }) => {
    await page.goto('/');

    // Wait for the app to load and the body to be present
    const body = page.locator('body');
    await expect(body).toBeVisible();

    // The frontend should have initialized without crashing 
    // waiting on real Tauri IPC since we mocked it.
    await page.waitForTimeout(1000); // Small pause to ensure rendering
  });
});
