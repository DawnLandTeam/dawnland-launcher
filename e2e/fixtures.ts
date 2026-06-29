import { test as base, chromium, type Page, type Browser } from '@playwright/test';
import { spawn, execSync, type ChildProcess } from 'child_process';
import path from 'path';
import fs from 'fs';

async function forceKillChildProcess(childProcess: ChildProcess) {
  if (!childProcess.pid) return;
  if (process.platform === 'win32') {
    try {
      // nosemgrep: javascript.lang.security.detect-child-process
      execSync(`taskkill /pid ${childProcess.pid} /T /F`, { stdio: 'ignore' });
    } catch (e) {
      // Ignore if process is already dead
    }
  } else {
    // Attempt graceful shutdown first
    childProcess.kill('SIGTERM');
    await new Promise(resolve => setTimeout(resolve, 1000));
    try {
      // Sending signal 0 checks if process still exists
      process.kill(childProcess.pid, 0);
      childProcess.kill('SIGKILL');
    } catch (e) {
      // Process already dead
    }
  }
}

// Define worker-level fixtures
type WorkerFixtures = {
  sharedApp: { page: Page };
};

// Export a custom test fixture that provisions a clean Tauri process per worker
export const test = base.extend<{ page: Page }, WorkerFixtures>({
  sharedApp: [
    async ({}, use, workerInfo) => {
      // Explicitly parse and sanitize the worker index to ensure no command injection taint exists
      const safeWorkerIndex = parseInt(String(workerInfo.workerIndex), 10);
      if (isNaN(safeWorkerIndex)) throw new Error("Invalid worker index");

      const port = 9222 + safeWorkerIndex; // Ensure unique port per worker
      
      // 1. Create a unique sandbox directory for this worker (persists across tests in the same worker)
      const e2eTempDir = path.resolve(process.cwd(), 'e2e', '.temp', `worker-${safeWorkerIndex}`);
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
      // (Security Note: sandboxExePath is locally constructed and isolated from user input)
      const tauriLogPath = path.join(e2eTempDir, `tauri-worker-${safeWorkerIndex}.log`);
      const tauriLogStream = fs.createWriteStream(tauriLogPath, { flags: 'a' });

      // nosemgrep: javascript.lang.security.detect-child-process
      const childProcess: ChildProcess = spawn(sandboxExePath, [], { 
        env,
        cwd: e2eTempDir,
        stdio: ['ignore', 'pipe', 'pipe']
      });

      if (childProcess.stdout) {
        childProcess.stdout.pipe(tauriLogStream);
      }

      if (childProcess.stderr) {
        childProcess.stderr.pipe(tauriLogStream);
      }

      childProcess.on('close', () => {
        tauriLogStream.end();
      });

      // 5. Wait for the CDP port to become available
      let browser: Browser | null = null;
      const maxRetries = 40;
      const retryDelay = 500;
      let lastError: unknown = null;

      for (let i = 0; i < maxRetries; i++) {
        try {
          // Force IPv4 loopback to avoid ECONNREFUSED ::1 issues on Node 17+
          browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
          break;
        } catch (e) {
          lastError = e;
          // eslint-disable-next-line no-console
          const errMsg = e instanceof Error ? e.message.replace(/\n/g, ' | ') : String(e);
          console.warn(`[e2e] Waiting for CDP port ${port}... (attempt ${i + 1}/${maxRetries}) - ${errMsg}`);
          await new Promise(resolve => setTimeout(resolve, retryDelay));
        }
      }

      if (!browser) {
        await forceKillChildProcess(childProcess);
        throw new Error(
          `Failed to connect to CDP port ${port} after ${maxRetries} attempts. ` +
          `Last error: ${lastError instanceof Error ? lastError.message : String(lastError)}`
        );
      }

      let defaultContext = browser.contexts()[0];
      let page = defaultContext?.pages()[0];
      let pageRetries = 20;
      
      while ((!defaultContext || !page) && pageRetries > 0) {
        await new Promise(resolve => setTimeout(resolve, 500));
        defaultContext = browser.contexts()[0];
        page = defaultContext?.pages()[0];
        pageRetries--;
      }

      if (!defaultContext || !page) {
        await forceKillChildProcess(childProcess);
        throw new Error(`Failed to attach to default CDP context/page after waiting. App may have crashed or failed to load Webview.`);
      }

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
      
      // Forcefully kill the entire process tree (especially critical on Windows to release file locks)
      await forceKillChildProcess(childProcess);
      
      // Give the process a moment to exit before deleting the folder
      await new Promise(resolve => setTimeout(resolve, 1500));
      
      try {
        fs.rmSync(e2eTempDir, { recursive: true, force: true });
      } catch (e) {
        // eslint-disable-next-line no-console
        console.warn(`[e2e] Failed to cleanup temp dir ${e2eTempDir}:`, e);
      }
    },
    { scope: 'worker' } // This makes it initialize once per worker!
  ],

  // Override the test-level `page` fixture to just return our shared worker page
  page: async ({ sharedApp }, use) => {
    await use(sharedApp.page);
  }
});

export { expect } from '@playwright/test';
