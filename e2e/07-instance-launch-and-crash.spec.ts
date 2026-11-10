import { test, expect, resolveSafePath } from './fixtures';
import fs from 'fs';
import path from 'path';
import { writeMockGameJar, writeBrokenModJar } from './fixtures/mock-assets';

test.describe.serial('Instance Launch, Prelaunch Warnings & Crash Capture (E2E)', () => {
  const launchInstanceName = 'E2E_Launch_Target';

  test.afterEach(async ({ page }) => {
    try {
      await page.evaluate(
        (id) => (window as any).__TAURI_INTERNALS__?.invoke?.('kill_instance', { versionId: id }),
        launchInstanceName
      );
    } catch {
      // ignore errors if page or instance is already closed
    }
  });

  async function selectInstanceOnHome(page: any, instanceName: string) {
    const instanceSection = page.locator('label:has-text("选择实例"), label:has-text("Select Instance")').locator('..');
    const text = await instanceSection.textContent();
    if (!text?.includes(instanceName)) {
      const trigger = instanceSection.locator('button').first();
      await trigger.click();
      await page.locator(`text=${instanceName}`).last().click();
      await page.waitForTimeout(300);
    }
  }

  test('1. Setup mock instance with compiled runnable Minecraft jar', async ({ page }, testInfo) => {
    const safeWorkerIndex = parseInt(String(testInfo.workerIndex), 10);
    const e2eTempDir = resolveSafePath(process.cwd(), 'e2e', '.temp', `worker-${safeWorkerIndex}`);
    const mockInstanceDir = resolveSafePath(e2eTempDir, '.minecraft', 'versions', launchInstanceName);
    const parentVersionDir = resolveSafePath(e2eTempDir, '.minecraft', 'versions', '1.21.1');
    
    fs.mkdirSync(mockInstanceDir, { recursive: true });
    fs.mkdirSync(parentVersionDir, { recursive: true });

    // Seed runnable mock jar in both target instance and parent 1.21.1 directory
    const jarPath = resolveSafePath(mockInstanceDir, `${launchInstanceName}.jar`);
    const parentJarPath = resolveSafePath(parentVersionDir, '1.21.1.jar');
    writeMockGameJar(jarPath);
    writeMockGameJar(parentJarPath);

    // Create parent 1.21.1 version metadata
    fs.writeFileSync(resolveSafePath(parentVersionDir, '1.21.1.json'), JSON.stringify({
      id: "1.21.1",
      time: "2026-01-01T00:00:00+00:00",
      releaseTime: "2026-01-01T00:00:00+00:00",
      type: "release",
      mainClass: "net.minecraft.client.main.Main",
      libraries: []
    }));

    // Create target version metadata pointing to our Main class and inheriting from 1.21.1
    fs.writeFileSync(resolveSafePath(mockInstanceDir, `${launchInstanceName}.json`), JSON.stringify({
      id: launchInstanceName,
      inheritsFrom: "1.21.1",
      time: "2026-01-01T00:00:00+00:00",
      releaseTime: "2026-01-01T00:00:00+00:00",
      type: "release",
      mainClass: "net.minecraft.client.main.Main",
      libraries: []
    }));

    // Seed dlml.json with showGameLog enabled
    fs.writeFileSync(resolveSafePath(mockInstanceDir, 'dlml.json'), JSON.stringify({
      name: launchInstanceName,
      mcVersion: "1.21.1",
      loaderType: "Vanilla",
      showGameLog: true,
      status: "ready"
    }));

    // Seed an offline account to ensure play button is active
    const accountsNav = page.locator('a[href="/accounts"]').first();
    await accountsNav.click({ timeout: 10000 });
    const addAccountBtn = page.locator('button:has-text("添加账号"), button:has-text("Add Account")').first();
    if (await addAccountBtn.isVisible()) {
      await addAccountBtn.click();
      await page.locator('button:has-text("离线账号"), button:has-text("Offline")').first().click();
      await page.locator('input[placeholder*="输入用户名"], input[placeholder*="Enter username"]').fill('LaunchTester');
      await page.locator('button:has-text("添加"), button:has-text("Add")').last().click();
      await expect(page.locator('text=LaunchTester').first()).toBeVisible({ timeout: 5000 });
    }

    // Navigate to instances view to trigger backend scan and registration
    const instancesNav = page.locator('a[href="/instances"]').first();
    await instancesNav.click({ timeout: 10000 });
    await expect(page.locator(`text=${launchInstanceName}`).first()).toBeVisible({ timeout: 15000 });

    // Go to home view and verify the Play button is ready
    const homeNav = page.locator('a[href="/"]').first();
    await homeNav.click({ timeout: 10000 });
    await page.waitForLoadState('domcontentloaded');
    await selectInstanceOnHome(page, launchInstanceName);
    await expect(page.locator('button:has-text("启动"), button:has-text("Play")').first()).toBeVisible({ timeout: 10000 });
  });

  test('2. Prelaunch check warning modal verification', async ({ page }, testInfo) => {
    const safeWorkerIndex = parseInt(String(testInfo.workerIndex), 10);
    const e2eTempDir = resolveSafePath(process.cwd(), 'e2e', '.temp', `worker-${safeWorkerIndex}`);
    const modsDir = resolveSafePath(e2eTempDir, '.minecraft', 'versions', launchInstanceName, 'mods');
    fs.mkdirSync(modsDir, { recursive: true });

    // Create a mock mod jar with fabric.mod.json that depends on a non-existent mod
    const brokenModJar = path.join(modsDir, 'broken-mod.jar');
    writeBrokenModJar(brokenModJar);

    const homeNav = page.locator('a[href="/"]').first();
    await homeNav.click({ timeout: 10000 });
    await selectInstanceOnHome(page, launchInstanceName);

    // Click Play button
    const playBtn = page.locator('button:has-text("启动"), button:has-text("Play")').first();
    await expect(playBtn).toBeVisible({ timeout: 10000 });
    await playBtn.click();

    // Verify Prelaunch Warning Dialog pops up
    await expect(page.getByText(/前置依赖缺失|Pre-launch Warning/)).toBeVisible({ timeout: 10000 });
    await expect(page.getByText('missing_library_mod')).toBeVisible({ timeout: 5000 });

    // Click "取消" / "Cancel"
    const cancelBtn = page.locator('button:has-text("取消"), button:has-text("Cancel")').first();
    await cancelBtn.click();
    await expect(page.getByText(/前置依赖缺失|Pre-launch Warning/)).not.toBeVisible({ timeout: 5000 });

    // Clean up the broken mod
    fs.unlinkSync(brokenModJar);
  });

  test('3. Launch game, verify log window, running status & kill instance', async ({ page }) => {
    const homeNav = page.locator('a[href="/"]').first();
    await homeNav.click({ timeout: 10000 });

    // Click Play button
    const playBtn = page.locator('button:has-text("启动"), button:has-text("Play")').first();
    await expect(playBtn).toBeVisible({ timeout: 10000 });
    await playBtn.click();

    // Game log output modal should appear because showGameLog is true
    await expect(page.getByText(/游戏输出|Game Output/)).toBeVisible({ timeout: 15000 });

    // Verify process logs from our compiled Main class
    await expect(page.getByText('Backend library: LWJGL 3.3.3')).toBeVisible({ timeout: 15000 });
    await expect(page.getByText('Setting user: LaunchTester')).toBeVisible({ timeout: 15000 });

    // Close the game log window overlay
    const closeLogBtn = page.locator('div:has(> h3:has-text("游戏输出"), > h3:has-text("Game Output")) button').first();
    if (await closeLogBtn.isVisible()) {
      await closeLogBtn.click();
      await page.waitForTimeout(300);
    }

    // Kill the game cleanly via backend invoke
    await page.evaluate(
      (id) => (window as any).__TAURI_INTERNALS__.invoke('kill_instance', { versionId: id }),
      launchInstanceName
    );

    // After stopping, button should revert to "启动" / "Play"
    await expect(page.locator('button:has-text("启动"), button:has-text("Play")').first()).toBeVisible({ timeout: 15000 });

    // In case killing the process triggered CrashReportModal, dismiss it so tests stay clean
    const crashCloseBtn = page.locator('div:has(h3:has-text("游戏已崩溃"), h3:has-text("Game Crashed")) button').first();
    try {
      await crashCloseBtn.waitFor({ state: 'visible', timeout: 2000 });
      await crashCloseBtn.click();
      await page.waitForTimeout(300);
    } catch {
      // modal did not appear or was already dismissed
    }
  });

  test('4. Crash report capture on non-zero exit code', async ({ page }, testInfo) => {
    const safeWorkerIndex = parseInt(String(testInfo.workerIndex), 10);
    const e2eTempDir = resolveSafePath(process.cwd(), 'e2e', '.temp', `worker-${safeWorkerIndex}`);
    const configPath = resolveSafePath(e2eTempDir, '.minecraft', 'versions', launchInstanceName, 'dlml.json');

    // Configure -De2e.crash=true in dlml.json as Vec<String>
    const config = JSON.parse(fs.readFileSync(configPath, 'utf-8'));
    config.jvm_args_extra = ['-De2e.crash=true'];
    config.jvmArgsExtra = ['-De2e.crash=true'];
    fs.writeFileSync(configPath, JSON.stringify(config));

    // Dismiss any leftover modal if present
    const leftoverCrash = page.locator('div:has(h3:has-text("游戏已崩溃"), h3:has-text("Game Crashed")) button').first();
    if (await leftoverCrash.isVisible()) {
      await leftoverCrash.click();
      await page.waitForTimeout(300);
    }
    const leftoverLog = page.locator('div:has(> h3:has-text("游戏输出"), > h3:has-text("Game Output")) button').first();
    if (await leftoverLog.isVisible()) {
      await leftoverLog.click();
      await page.waitForTimeout(300);
    }

    // Return to home if not already on it
    const homeNav = page.locator('a[href="/"]').first();
    const isActiveHome = (await homeNav.getAttribute('class'))?.includes('router-link-active');
    if (!isActiveHome) {
      await homeNav.click({ timeout: 10000 });
    }

    const playBtn = page.locator('button:has-text("启动"), button:has-text("Play")').first();
    await expect(playBtn).toBeVisible({ timeout: 10000 });
    await playBtn.click();

    // Verify Crash Report Modal pops up
    await expect(page.getByText(/游戏已崩溃|Game Crashed/)).toBeVisible({ timeout: 20000 });
    await expect(page.getByText(/Fatal: E2E simulated game crash triggered|E2E simulated game crash triggered/)).toBeVisible({ timeout: 10000 });

    // Close crash report modal specifically
    const closeCrashBtn = page.locator('div:has(h3:has-text("游戏已崩溃"), h3:has-text("Game Crashed")) button').first();
    await closeCrashBtn.click();
    await page.waitForTimeout(300);

    // Also close game log modal if open
    const closeLogModal = page.locator('div:has(> h3:has-text("游戏输出"), > h3:has-text("Game Output")) button').first();
    if (await closeLogModal.isVisible()) {
      await closeLogModal.click();
      await page.waitForTimeout(300);
    }

    // Verify Crash History has automatically recorded the crash
    const instancesNav = page.locator('a[href="/instances"]').first();
    await instancesNav.click({ timeout: 10000 });
    await page.locator(`text=${launchInstanceName}`).first().click();

    const crashTab = page.locator('[role="tab"]:has-text("崩溃历史"), [role="tab"]:has-text("Crash History"), button:has-text("崩溃历史"), button:has-text("Crash History")').first();
    await crashTab.click({ timeout: 10000 });

    // Exit code 1 and summary should be visible in history entry
    await expect(page.getByText(/Exit Code: 1|退出代码: 1|代码: 1/).first()).toBeVisible({ timeout: 10000 });

    // Expand the crash entry to view full crash summary log
    const crashEntry = page.locator('div.cursor-pointer', { hasText: 'Exit Code: 1' }).first();
    await crashEntry.click();
    await expect(page.getByText(/Fatal: E2E simulated game crash triggered|E2E simulated game crash triggered/).first()).toBeVisible({ timeout: 10000 });
  });
});
