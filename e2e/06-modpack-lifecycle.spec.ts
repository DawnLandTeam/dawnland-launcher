import { test, expect, resolveSafePath } from './fixtures';
import fs from 'fs';

test.describe.serial('Modpack Lifecycle & Downgrade Protection (E2E)', () => {
  const modpackInstanceName = 'E2E_Modpack_Lifecycle';

  test('1. Setup mock modpack instance with v1.0.0', async ({ page }, testInfo) => {
    const safeWorkerIndex = parseInt(String(testInfo.workerIndex), 10);
    const e2eTempDir = resolveSafePath(process.cwd(), 'e2e', '.temp', `worker-${safeWorkerIndex}`);
    const mockInstanceDir = resolveSafePath(e2eTempDir, '.minecraft', 'versions', modpackInstanceName);

    fs.mkdirSync(mockInstanceDir, { recursive: true });
    fs.writeFileSync(resolveSafePath(mockInstanceDir, `${modpackInstanceName}.json`), JSON.stringify({
      id: modpackInstanceName,
      inheritsFrom: "1.20.1",
      libraries: [{ name: "net.fabricmc:fabric-loader:0.15.0" }]
    }));

    // Seed dlml.json with modpack metadata (v15.0.0-beta.3, Modrinth source)
    fs.writeFileSync(resolveSafePath(mockInstanceDir, 'dlml.json'), JSON.stringify({
      name: modpackInstanceName,
      mcVersion: "1.20.1",
      loaderType: "Fabric",
      loaderVersion: "0.15.0",
      modpackType: "Modrinth",
      modpackVersion: "15.0.0-beta.3",
      modpackProjectId: "fabulously-optimized",
      status: "ready"
    }));

    const homeNav = page.locator('a[href="/"]').first();
    await homeNav.click({ timeout: 10000 });
    const instancesNav = page.locator('a[href="/instances"]').first();
    await instancesNav.click({ timeout: 10000 });
    await expect(page.locator(`text=${modpackInstanceName}`).first()).toBeVisible({ timeout: 15000 });
  });

  test('2. Verify Update Modpack tab is visible for modpack instances', async ({ page }) => {
    const instancesNav = page.locator('a[href="/instances"]').first();
    await instancesNav.click({ timeout: 10000 });
    await page.locator(`text=${modpackInstanceName}`).first().click();

    // The "更新整合包" / "Update Modpack" tab should be rendered in the sidebar
    const updateModpackTab = page.locator('[role="tab"]:has-text("更新整合包"), [role="tab"]:has-text("Update Modpack"), button:has-text("更新整合包"), button:has-text("Update Modpack")').first();
    await expect(updateModpackTab).toBeVisible({ timeout: 10000 });
  });

  test('3. Verify Downgrade Warning dialog triggers and can be cancelled', async ({ page }) => {
    // Navigate to instance if needed
    const updateModpackTab = page.locator('[role="tab"]:has-text("更新整合包"), [role="tab"]:has-text("Update Modpack"), button:has-text("更新整合包"), button:has-text("Update Modpack")').first();
    if (!await updateModpackTab.isVisible({ timeout: 3000 }).catch(() => false)) {
      const instancesNav = page.locator('a[href="/instances"]').first();
      await instancesNav.click({ timeout: 10000 });
      await page.locator(`text=${modpackInstanceName}`).first().click();
    }
    await expect(updateModpackTab).toBeVisible({ timeout: 10000 });

    // Mock get_modrinth_modpack_versions to avoid external network latency and rate limiting
    await updateModpackTab.click();

    // Versions list should load for fabulously-optimized
    // Find the downgrade button (Modrinth API can take ~15s depending on CDN routing)
    const downgradeBtn = page.getByRole('button', { name: /降级|Downgrade/ }).first();
    await expect(downgradeBtn).toBeVisible({ timeout: 45000 });
    await downgradeBtn.click();

    // Verify the downgrade alert dialog appears
    await expect(page.getByText(/降级提示|整合包降级风险提示与确认|Downgrade Warning/)).toBeVisible({ timeout: 10000 });

    // Click "取消" to test cancellation
    const cancelBtn = page.getByRole('button', { name: /取消|Cancel/ }).last();
    await cancelBtn.click();

    // Dialog should close
    await expect(page.getByText(/降级提示|整合包降级风险提示与确认|Downgrade Warning/)).not.toBeVisible({ timeout: 5000 });

    // Close the versions dialog by pressing Escape or clicking its X button
    await page.keyboard.press('Escape');
    const closeDialogBtn = page.locator('div[role="dialog"] button:has(.lucide-x)').first();
    if (await closeDialogBtn.isVisible({ timeout: 1000 }).catch(() => false)) {
      await closeDialogBtn.click();
    }

    // Switch back to Settings tab to ensure cleanly unmounted
    const settingsTab = page.locator('[role="tab"]:has-text("设置"), [role="tab"]:has-text("Settings"), button:has-text("设置"), button:has-text("Settings")').first();
    await expect(settingsTab).toBeVisible({ timeout: 5000 });
    await settingsTab.click();
  });

  test('4. Modpack Export Modal flow', async ({ page }) => {
    // Ensure any open dialog is closed
    const closeDialogBtn = page.locator('div[role="dialog"] button:has(.lucide-x), div[role="dialog"] button:has-text("Close")').first();
    if (await closeDialogBtn.isVisible({ timeout: 2000 }).catch(() => false)) {
      await closeDialogBtn.click();
    }

    // If not on instance management view, navigate
    const exportBtn = page.getByRole('button', { name: /导出整合包|Export Modpack/ }).first();
    if (!await exportBtn.isVisible()) {
      const instancesNav = page.locator('a[href="/instances"]').first();
      await instancesNav.click({ timeout: 10000 });
      await page.locator(`text=${modpackInstanceName}`).first().click();
    }

    await expect(exportBtn).toBeVisible({ timeout: 10000 });
    await exportBtn.click();

    // Export dialog should appear
    const exportDialog = page.locator('div[role="dialog"]').first();
    await expect(exportDialog).toBeVisible({ timeout: 10000 });

    // Modpack name input should be pre-filled with instance name
    const nameInput = exportDialog.locator('input').first();
    await expect(nameInput).toHaveValue(modpackInstanceName);

    // Verify format options: Modrinth (.mrpack) and CurseForge (.zip)
    await expect(exportDialog.getByText(/Modrinth/)).toBeVisible();
    await expect(exportDialog.getByText(/\.mrpack/)).toBeVisible();
    await expect(exportDialog.getByText(/CurseForge/)).toBeVisible();
    await expect(exportDialog.getByText(/\.zip/)).toBeVisible();

    // Close export dialog via footer Cancel button to avoid any floating toast interference at top-right
    const cancelBtn = exportDialog.locator('button:has-text("Cancel"), button:has-text("取消")').first();
    if (await cancelBtn.isVisible({ timeout: 2000 }).catch(() => false)) {
      await cancelBtn.click();
    } else {
      const closeBtn = exportDialog.locator('button:has(.lucide-x), button:has-text("Close")').first();
      await closeBtn.click({ force: true }).catch(() => {});
    }
    await expect(exportDialog).not.toBeVisible({ timeout: 5000 });
  });
});
