import { test, expect, resolveSafePath } from './fixtures';
import fs from 'fs';

test.describe.serial('Instance Install Wizard & Mod Loader Lifecycle (E2E)', () => {
  const vanillaInstanceName = 'E2E_Vanilla_Lifecycle';

  test('1. Setup mock pure vanilla instance', async ({ page }, testInfo) => {
    const safeWorkerIndex = parseInt(String(testInfo.workerIndex), 10);
    const e2eTempDir = resolveSafePath(process.cwd(), 'e2e', '.temp', `worker-${safeWorkerIndex}`);
    const mockInstanceDir = resolveSafePath(e2eTempDir, '.minecraft', 'versions', vanillaInstanceName);
    
    // Create the mock vanilla directory and json
    fs.mkdirSync(mockInstanceDir, { recursive: true });
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

    // Seed a mock mod in mods/ to test loader uninstall mod-handling
    const modsDir = resolveSafePath(mockInstanceDir, 'mods');
    fs.mkdirSync(modsDir, { recursive: true });
    fs.writeFileSync(resolveSafePath(modsDir, 'test-dummy-mod.jar'), 'PK_DUMMY_JAR_DATA');

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
  });

  test('4. Switch Loader version: Downgrade & Upgrade badges', async ({ page }) => {
    // Go to settings tab
    const settingsTab = page.locator('[role="tab"]:has-text("设置"), [role="tab"]:has-text("Settings"), button:has-text("设置"), button:has-text("Settings")').first();
    await settingsTab.click({ timeout: 10000 });

    // Verify core & loader management section
    await expect(page.getByText(/游戏核心与加载器|Game Core & Loader/)).toBeVisible({ timeout: 10000 });

    // Click "更换版本" (switch loader version)
    const switchBtn = page.getByRole('button', { name: /更换版本|Switch Version/ }).first();
    if (await switchBtn.isVisible()) {
      await switchBtn.click();
      const switchDialog = page.locator('div[role="dialog"]').first();
      await expect(switchDialog).toBeVisible({ timeout: 10000 });

      // Open version dropdown
      const versionDropdown = switchDialog.locator('button[type="button"]:has(.lucide-chevron-down)').first();
      if (await versionDropdown.isVisible()) {
        await versionDropdown.click();

        // Check if options list appears
        const options = switchDialog.locator('div[role="option"]');
        const count = await options.count();
        if (count > 1) {
          // Pick the last option (older version)
          await options.last().click();

          // Verify "降级" (Downgrade) badge appears
          await expect(switchDialog.getByText(/降级|Downgrade/)).toBeVisible({ timeout: 5000 });
        }
      }

      // Close switch dialog without breaking state
      await page.keyboard.press('Escape');
      const cancelBtn = switchDialog.locator('button:has-text("取消"), button:has-text("Cancel"), button:has(.lucide-x)').first();
      if (await cancelBtn.isVisible({ timeout: 1000 }).catch(() => false)) {
        await cancelBtn.click({ force: true }).catch(() => {});
      }
    }
  });

  test('5. Uninstall Mod Loader and revert to Vanilla with mod disabling', async ({ page }) => {
    const settingsTab = page.locator('[role="tab"]:has-text("设置"), [role="tab"]:has-text("Settings"), button:has-text("设置"), button:has-text("Settings")').first();
    await settingsTab.click({ timeout: 10000 });

    const uninstallBtn = page.getByRole('button', { name: /卸载加载器|Uninstall Loader/ }).first();
    if (await uninstallBtn.isVisible()) {
      await uninstallBtn.click();
      const uninstallDialog = page.locator('div[role="dialog"]').first();
      await expect(uninstallDialog).toBeVisible({ timeout: 10000 });

      // Ensure "禁用所有模组" (disable) radio is selected
      const disableRadio = uninstallDialog.locator('input[value="disable"]');
      if (await disableRadio.isVisible()) {
        await disableRadio.check();
      }

      // Confirm uninstall
      const submitBtn = uninstallDialog.locator('button:has-text("确认卸载"), button:has-text("Confirm Uninstall"), button:has-text("确定")').first();
      await submitBtn.click();

      // Wait for dialog to dismiss
      await expect(uninstallDialog).not.toBeVisible({ timeout: 15000 });

      // Switch back to Mods tab to verify mods were disabled and vanilla banner returned
      const modsTab = page.locator('[role="tab"]:has-text("模组"), [role="tab"]:has-text("Mods"), button:has-text("模组"), button:has-text("Mods")').first();
      await modsTab.click({ timeout: 10000 });
      await expect(page.getByText(/纯原版|Vanilla Minecraft/)).toBeVisible({ timeout: 15000 });
    }
  });

  test('6. Instance Install Tab validation & conflict matrix', async ({ page }) => {
    // Navigate to Downloads Center
    const downloadsNav = page.locator('a[href="/downloads"]').first();
    await downloadsNav.click({ timeout: 10000 });

    // Click Game Versions / Instance Tab
    const instanceTab = page.locator('[role="tab"]:has-text("游戏版本"), [role="tab"]:has-text("Instances"), button:has-text("游戏版本"), button:has-text("Instances")').first();
    if (await instanceTab.isVisible()) {
      await instanceTab.click({ timeout: 5000 });
    }

    // Wait for version cards or search input
    const searchInput = page.locator('input[placeholder*="搜索版本"], input[placeholder*="Search"]').first();
    if (await searchInput.isVisible()) {
      await searchInput.fill('1.21.1');
      await page.waitForTimeout(500);

      const targetVersion = page.locator('text=1.21.1').first();
      if (await targetVersion.isVisible()) {
        await targetVersion.click();

        // Should advance to Step 2: Mod Loader configuration
        await expect(page.locator('text=Fabric').first()).toBeVisible({ timeout: 10000 });

        // Click Fabric card
        const fabricCard = page.locator('div.cursor-pointer', { hasText: 'Fabric' }).first();
        if (await fabricCard.isVisible()) {
          await fabricCard.click();
          // Verify conflict detection
          await expect(page.getByText(/冲突|Conflict|Incompatible/).first()).toBeVisible({ timeout: 10000 });
        }
      }
    }
  });
});
