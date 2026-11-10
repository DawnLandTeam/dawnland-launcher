import { test, expect } from './fixtures';

test.describe('Settings & Updater (E2E)', () => {
  test('1. General Settings, Cache Cleaning and Language Switch', async ({ page }) => {
    await page.waitForLoadState('domcontentloaded');

    // Navigate to settings
    const settingsNav = page.locator('a[title="设置"], a[title="Settings"], a[href="/settings"]').first();
    await settingsNav.click({ timeout: 10000 });

    // Verify Settings page loaded (Common/General settings tab is selected by default)
    const generalTab = page.locator('[role="tab"]:has-text("常用设置"), [role="tab"]:has-text("General")').first();
    await expect(generalTab).toBeVisible({ timeout: 10000 });

    // Click "清理无引用缓存" / "Clean Unreferenced Cache"
    const cleanCacheBtn = page.locator('button:has-text("清理无引用缓存"), button:has-text("Clean Unreferenced Cache")').first();
    await expect(cleanCacheBtn).toBeVisible({ timeout: 10000 });
    await cleanCacheBtn.click();
    await page.waitForTimeout(300);

    // Verify language select trigger is visible
    const langSelect = page.locator('div.relative button:has-text("English"), div.relative button:has-text("简体中文")').first();
    await expect(langSelect).toBeVisible({ timeout: 5000 });
  });

  test('2. About Tab & Check for Updates prompt', async ({ page }) => {
    // Navigate to settings if not already there
    const settingsNav = page.locator('a[title="设置"], a[title="Settings"], a[href="/settings"]').first();
    await settingsNav.click({ timeout: 10000 });

    // Switch to "关于" / "About" tab
    const aboutTab = page.locator('[role="tab"]:has-text("关于"), [role="tab"]:has-text("About")').first();
    await aboutTab.click({ timeout: 5000 });

    // Verify About view contents
    await expect(page.locator('h2:has-text("Dawnland Launcher")').first()).toBeVisible({ timeout: 10000 });

    // Check for "Check for Updates" button
    const checkUpdateBtn = page.locator('button:has-text("检查更新"), button:has-text("Check for Updates")').first();
    await expect(checkUpdateBtn).toBeVisible({ timeout: 10000 });

    // Handle alert dialog if one is shown during update check
    page.once('dialog', async (dialog) => {
      await dialog.dismiss().catch(() => {});
    });

    await checkUpdateBtn.click();

    // Dismiss update modal if prompted by the check so button returns to stable state
    const dismissUpdateBtn = page.locator('.z-\\[100\\] button:has-text("稍后再说"), .z-\\[100\\] button:has-text("Later")').first();
    try {
      if (await dismissUpdateBtn.isVisible({ timeout: 2000 })) {
        await dismissUpdateBtn.click({ force: true });
        await dismissUpdateBtn.waitFor({ state: 'hidden', timeout: 5000 });
      }
    } catch {}

    await expect(checkUpdateBtn).toBeEnabled({ timeout: 15000 });
  });
});
