import { test as base, chromium, type Page, type Browser } from '@playwright/test';
import { spawn, type ChildProcess } from 'child_process';
import path from 'path';
import fs from 'fs';
import { randomUUID } from 'crypto';

// Define worker-level fixtures
type WorkerFixtures = {
  sharedApp: { page: Page };
};

// Export a custom test fixture that provisions a clean Tauri process per worker
export const test = base.extend<{ page: Page }, WorkerFixtures>({
  sharedApp: [
    async ({}, use, workerInfo) => {
      const port = 9222 + workerInfo.workerIndex; // Ensure unique port per worker
      
      // 1. Create a unique sandbox directory for this worker (persists across tests in the same worker)
      const e2eTempDir = path.resolve(process.cwd(), 'e2e', '.temp', `worker-${workerInfo.workerIndex}`);
      // Clean up previous runs if any
      if (fs.existsSync(e2eTempDir)) {
        fs.rmSync(e2eTempDir, { recursive: true, force: true });
      }
      fs.mkdirSync(e2eTempDir, { recursive: true });

      // 2. Copy the executable to the sandbox directory to isolate data
      const originalExePath = path.resolve(process.cwd(), 'src-tauri', 'target', 'release', 'DLML.exe');
      if (!fs.existsSync(originalExePath)) {
        throw new Error(`Executable not found at ${originalExePath}. Did you run 'pnpm run build:e2e'?`);
      }
      
      const sandboxExePath = path.join(e2eTempDir, 'DLML.exe');
      fs.copyFileSync(originalExePath, sandboxExePath);

      // 3. Set environment variable to open WebView2 debugging port
      const env = { 
        ...process.env,
        WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}` 
      };
      
      // 4. Start the copied executable with sandbox as CWD
      const childProcess: ChildProcess = spawn(sandboxExePath, [], { 
        env,
        cwd: e2eTempDir,
        stdio: 'ignore' 
      });

      // 5. Wait for the CDP port to become available
      let browser: Browser | null = null;
      const maxRetries = 40;
      const retryDelay = 500;
      for (let i = 0; i < maxRetries; i++) {
        try {
          browser = await chromium.connectOverCDP(`http://localhost:${port}`);
          break;
        } catch (e) {
          await new Promise(resolve => setTimeout(resolve, retryDelay));
        }
      }

      if (!browser) {
        childProcess.kill();
        throw new Error(`Failed to connect to CDP port ${port} after ${maxRetries * retryDelay}ms`);
      }

      const defaultContext = browser.contexts()[0];
      const page = defaultContext.pages()[0];

      // Auto-dismiss Privacy Policy modal if it appears at any time
      await page.addLocatorHandler(page.locator('.z-\\[100\\] button:has-text("拒绝并继续"), .z-\\[100\\] button:has-text("Agree")'), async () => {
        await page.locator('.z-\\[100\\] button:has-text("拒绝并继续"), .z-\\[100\\] button:has-text("Agree")').first().click();
      });

      // Auto-dismiss Updater modal if it appears at any time
      await page.addLocatorHandler(page.locator('.z-\\[100\\] button:has-text("稍后再说"), .z-\\[100\\] button:has-text("Later")'), async () => {
        await page.locator('.z-\\[100\\] button:has-text("稍后再说"), .z-\\[100\\] button:has-text("Later")').first().click();
      });

      // 6. Provide the shared app to the tests running in this worker
      await use({ page });

      // 7. Cleanup after all tests in this worker complete
      await browser.close();
      childProcess.kill();
      
      // Give the process a moment to exit before deleting the folder
      setTimeout(() => {
        try {
          fs.rmSync(e2eTempDir, { recursive: true, force: true });
        } catch (e) {
          console.warn(`Failed to cleanup temp dir ${e2eTempDir}:`, e);
        }
      }, 1500);
    },
    { scope: 'worker' } // This makes it initialize once per worker!
  ],

  // Override the test-level `page` fixture to just return our shared worker page
  page: async ({ sharedApp }, use) => {
    await use(sharedApp.page);
  }
});

export { expect } from '@playwright/test';
