import { test, expect, resolveSafePath } from './fixtures';
import fs from 'fs';
import { writeMockGameJar } from './fixtures/mock-assets';

test.describe.serial('Instance Install Wizard & Mod Loader Lifecycle (E2E)', () => {
  const vanillaInstanceName = 'E2E_Vanilla_Lifecycle';

  test('1. Setup mock pure vanilla instance', async ({ page }, testInfo) => {
    const safeWorkerIndex = parseInt(String(testInfo.workerIndex), 10);
    const e2eTempDir = resolveSafePath(process.cwd(), 'e2e', '.temp', `worker-${safeWorkerIndex}`);
    const mockInstanceDir = resolveSafePath(e2eTempDir, '.minecraft', 'versions', vanillaInstanceName);
    const parentVersionDir = resolveSafePath(e2eTempDir, '.minecraft', 'versions', '1.21.1');
    
    // Create the mock vanilla directory and json
    fs.mkdirSync(mockInstanceDir, { recursive: true });
    fs.mkdirSync(parentVersionDir, { recursive: true });

    writeMockGameJar(resolveSafePath(mockInstanceDir, `${vanillaInstanceName}.jar`));
    writeMockGameJar(resolveSafePath(parentVersionDir, '1.21.1.jar'));

    fs.writeFileSync(resolveSafePath(parentVersionDir, '1.21.1.json'), JSON.stringify({
      id: "1.21.1",
      time: "2026-01-01T00:00:00+00:00",
      releaseTime: "2026-01-01T00:00:00+00:00",
      type: "release",
      mainClass: "net.minecraft.client.main.Main",
      libraries: []
    }));

    fs.writeFileSync(resolveSafePath(mockInstanceDir, `${vanillaInstanceName}.json`), JSON.stringify({
      id: vanillaInstanceName,
      time: "2026-01-01T00:00:00+00:00",
      releaseTime: "2026-01-01T00:00:00+00:00",
      type: "release",
      mainClass: "net.minecraft.client.main.Main",
      inheritsFrom: "1.21.1",
      libraries: []
    }));

    // Seed dlml.json with Vanilla loader type
    fs.writeFileSync(resolveSafePath(mockInstanceDir, 'dlml.json'), JSON.stringify({
      name: vanillaInstanceName,
      mcVersion: "1.21.1",
      loaderType: "Vanilla",
      status: "ready"
    }));

    // Seed mock mods in mods/ to test loader uninstall mod-handling and skip auto-install network queries
    const modsDir = resolveSafePath(mockInstanceDir, 'mods');
    fs.mkdirSync(modsDir, { recursive: true });
    fs.writeFileSync(resolveSafePath(modsDir, 'test-dummy-mod.jar'), 'PK_DUMMY_JAR_DATA');
    fs.writeFileSync(resolveSafePath(modsDir, 'fabric-api-0.100.0.jar'), 'PK_FABRIC_API');

    // Navigate to instances view
    const homeNav = page.locator('a[href="/"]').first();
    await homeNav.click({ timeout: 10000 });
    const instancesNav = page.locator('a[href="/instances"]').first();
    await instancesNav.click({ timeout: 10000 });
    await expect(page.locator(`text=${vanillaInstanceName}`).first()).toBeVisible({ timeout: 15000 });
  });

  test('2. Verify Vanilla warning banner in Mods Tab', async ({ page }) => {
    const instancesNav = page.locator('a[href="/instances"]').first();
    await instancesNav.click({ timeout: 10000 });
    await page.locator(`text=${vanillaInstanceName}`).first().click();

    // Go to Mods tab
    const modsTab = page.locator('[role="tab"]:has-text("模组"), [role="tab"]:has-text("Mods"), button:has-text("模组"), button:has-text("Mods")').first();
    await modsTab.click({ timeout: 10000 });

    // Verify the vanilla warning banner is displayed
    await expect(page.getByText(/纯原版|Vanilla Minecraft/)).toBeVisible({ timeout: 10000 });
    const installLoaderBtn = page.getByRole('button', { name: /安装 Mod Loader|Install Mod Loader/ }).first();
    await expect(installLoaderBtn).toBeVisible({ timeout: 5000 });
  });

  test('3. Install Fabric Loader onto Vanilla instance', async ({ page }) => {
    // Click "安装 Mod Loader" in the banner
    const installLoaderBtn = page.getByRole('button', { name: /安装 Mod Loader|Install Mod Loader/ }).first();
    await installLoaderBtn.click({ timeout: 5000 });

    // Verify modal is open
    const dialog = page.locator('div[role="dialog"]').first();
    await expect(dialog).toBeVisible({ timeout: 10000 });

    // Click Fabric
    const fabricBtn = dialog.locator('button:has-text("Fabric")').first();
    await fabricBtn.click();

    // Wait for version dropdown to load available Fabric versions
    const versionSelect = dialog.locator('button[type="button"]:has(.lucide-chevron-down)').first();
    await expect(versionSelect).toBeVisible({ timeout: 15000 });

    // Click confirm / submit button
    const confirmBtn = dialog.locator('button:has-text("确认"), button:has-text("Confirm"), button:has-text("确定")').first();
    await expect(confirmBtn).toBeVisible({ timeout: 5000 });
    await confirmBtn.click();

    // Modal should close
    await expect(dialog).not.toBeVisible({ timeout: 15000 });

    // Navigate to Settings tab and wait for loader install task to finish
    const settingsTab = page.locator('[role="tab"]:has-text("设置"), [role="tab"]:has-text("Settings"), button:has-text("设置"), button:has-text("Settings")').first();
    await settingsTab.click({ timeout: 10000 });
    const switchBtn = page.getByRole('button', { name: /更换版本|Switch Version|Change Version/ }).first();
    await expect(switchBtn).toBeVisible({ timeout: 120000 });
    await expect(switchBtn).toBeEnabled({ timeout: 120000 });
  });

  test('4. Switch Loader version: Downgrade & Upgrade badges', async ({ page }) => {
    // Go to settings tab
    const settingsTab = page.locator('[role="tab"]:has-text("设置"), [role="tab"]:has-text("Settings"), button:has-text("设置"), button:has-text("Settings")').first();
    await settingsTab.click({ timeout: 10000 });

    // Verify core & loader management section
    await expect(page.getByText(/游戏核心与加载器|Game Core & Loader/)).toBeVisible({ timeout: 10000 });

    // Click "更换版本" (switch loader version)
    const switchBtn = page.getByRole('button', { name: /更换版本|Switch Version|Change Version/ }).first();
    await expect(switchBtn).toBeVisible({ timeout: 10000 });
    await expect(switchBtn).toBeEnabled({ timeout: 10000 });
    await switchBtn.click();

    const switchDialog = page.locator('div[role="dialog"]').first();
    await expect(switchDialog).toBeVisible({ timeout: 10000 });

    // Open version dropdown
    const versionDropdown = switchDialog.locator('button[type="button"]:has(.lucide-chevron-down)').first();
    await expect(versionDropdown).toBeVisible({ timeout: 10000 });
    await versionDropdown.click();

    // Check if options list appears
    const options = switchDialog.locator('div[role="option"]');
    await expect(options.first()).toBeVisible({ timeout: 10000 });
    const count = await options.count();
    expect(count).toBeGreaterThan(1);

    // Pick the last option (older version)
    await options.last().click();

    // Verify "降级" (Downgrade) badge appears
    await expect(switchDialog.getByText(/降级|Downgrade/)).toBeVisible({ timeout: 5000 });

    // Close switch dialog without breaking state
    const cancelBtn = switchDialog.getByRole('button', { name: /取消|Cancel/ }).first();
    await cancelBtn.click();
    await expect(switchDialog).not.toBeVisible({ timeout: 5000 });
  });

  test('5. Uninstall Mod Loader and revert to Vanilla with mod disabling', async ({ page }, testInfo) => {
    const settingsTab = page.locator('[role="tab"]:has-text("设置"), [role="tab"]:has-text("Settings"), button:has-text("设置"), button:has-text("Settings")').first();
    await settingsTab.click({ timeout: 10000 });

    const uninstallBtn = page.getByRole('button', { name: /卸载加载器|Uninstall Loader/ }).first();
    await expect(uninstallBtn).toBeVisible({ timeout: 10000 });
    await expect(uninstallBtn).toBeEnabled({ timeout: 10000 });
    await uninstallBtn.click();

    const uninstallDialog = page.locator('div[role="dialog"]').first();
    await expect(uninstallDialog).toBeVisible({ timeout: 10000 });

    // Ensure "禁用所有模组" (disable) radio is selected
    const disableRadio = uninstallDialog.locator('input[value="disable"]');
    await expect(disableRadio).toBeVisible({ timeout: 5000 });
    await disableRadio.check();

    // Confirm uninstall
    const submitBtn = uninstallDialog.locator('button:has-text("确认卸载"), button:has-text("Confirm Uninstall"), button:has-text("确定")').first();
    await expect(submitBtn).toBeVisible({ timeout: 5000 });
    await submitBtn.click();

    // Wait for dialog to dismiss
    await expect(uninstallDialog).not.toBeVisible({ timeout: 15000 });

    // Switch back to Mods tab to verify mods were disabled and vanilla banner returned
    const modsTab = page.locator('[role="tab"]:has-text("模组"), [role="tab"]:has-text("Mods"), button:has-text("模组"), button:has-text("Mods")').first();
    await modsTab.click({ timeout: 10000 });
    await expect(page.getByText(/纯原版|Vanilla Minecraft/)).toBeVisible({ timeout: 20000 });

    // Verify mod file on disk was renamed to .disable
    const safeWorkerIndex = parseInt(String(testInfo.workerIndex), 10);
    const e2eTempDir = resolveSafePath(process.cwd(), 'e2e', '.temp', `worker-${safeWorkerIndex}`);
    const modsDir = resolveSafePath(e2eTempDir, '.minecraft', 'versions', vanillaInstanceName, 'mods');
    expect(fs.existsSync(resolveSafePath(modsDir, 'test-dummy-mod.jar.disable'))).toBe(true);
    expect(fs.existsSync(resolveSafePath(modsDir, 'test-dummy-mod.jar'))).toBe(false);
  });

  test('6. Instance Install Tab validation & conflict matrix', async ({ page }) => {
    // Navigate to Downloads Center
    const downloadsNav = page.locator('a[href="/downloads"]').first();
    await downloadsNav.click({ timeout: 10000 });

    // Click Game Versions / Instance Tab
    const instanceTab = page.locator('[role="tab"]:has-text("游戏版本"), [role="tab"]:has-text("Instances"), button:has-text("游戏版本"), button:has-text("Instances")').first();
    await expect(instanceTab).toBeVisible({ timeout: 10000 });
    await instanceTab.click();

    // Fill search input
    const searchInput = page.locator('input[placeholder*="搜索版本"], input[placeholder*="Search"]').first();
    await expect(searchInput).toBeVisible({ timeout: 10000 });
    await searchInput.fill('1.21.1');
    await page.waitForTimeout(500);

    const targetVersion = page.locator('text=1.21.1').first();
    await expect(targetVersion).toBeVisible({ timeout: 10000 });
    await targetVersion.click();

    // Should advance to Step 2: Mod Loader configuration
    await expect(page.getByText(/Modular Instance Assembler|模块化实例组装/)).toBeVisible({ timeout: 10000 });

    // Click OptiFine card to trigger conflict with Fabric & NeoForge
    const optifineCard = page.locator('div.relative', { hasText: 'OptiFine' }).first();
    await optifineCard.click();

    // Verify conflict detection: incompatible reason must be displayed
    await expect(page.getByText(/与 OptiFine 冲突|Incompatible with OptiFine/).first()).toBeVisible({ timeout: 10000 });
  });
});
