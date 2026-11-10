import { test, expect, resolveSafePath } from './fixtures';
import fs from 'fs';

test.describe.serial('Fabric Mod Workflow (E2E)', () => {
  
  test('1. Setup mock instance', async ({ page }, testInfo) => {
    // Explicitly parse and sanitize the worker index
    const safeWorkerIndex = parseInt(String(testInfo.workerIndex), 10);
    const e2eTempDir = resolveSafePath(process.cwd(), 'e2e', '.temp', `worker-${safeWorkerIndex}`);
    const mockInstanceDir = resolveSafePath(e2eTempDir, '.minecraft', 'versions', 'E2E_Fabric_Test');
    
    // Create the mock directory and configuration
    fs.mkdirSync(mockInstanceDir, { recursive: true });
    fs.writeFileSync(resolveSafePath(mockInstanceDir, 'E2E_Fabric_Test.json'), JSON.stringify({
      id: "E2E_Fabric_Test",
      inheritsFrom: "1.20.1",
      libraries: [{ name: "net.fabricmc:fabric-loader:0.15.0" }]
    }));
    fs.writeFileSync(resolveSafePath(mockInstanceDir, 'dlml.json'), JSON.stringify({
      name: "E2E_Fabric_Test",
      mcVersion: "1.20.1",
      loaderType: "Fabric",
      loaderVersion: "0.15.0",
      status: "ready"
    }));
    
    // Navigate to instances page to trigger a fresh scan by the backend
    const homeNav = page.locator('a[href="/"]').first();
    await homeNav.click({ timeout: 10000 });
    const instancesNav = page.locator('a[href="/instances"]').first();
    await instancesNav.click({ timeout: 10000 });
    
    // Wait for the UI to reflect the newly seeded mock instance
    await expect(page.locator('text=E2E_Fabric_Test').first()).toBeVisible({ timeout: 15000 });
  });

  test('2. Install independent mod: JEI', async ({ page }, testInfo) => {
    const safeWorkerIndex = parseInt(String(testInfo.workerIndex), 10);
    const modsDir = resolveSafePath(process.cwd(), 'e2e', '.temp', `worker-${safeWorkerIndex}`, '.minecraft', 'versions', 'E2E_Fabric_Test', 'mods');

    const instancesNav = page.locator('a[href="/instances"]').first();
    await instancesNav.click({ timeout: 10000 });
    await page.locator('text=E2E_Fabric_Test').first().click();
    const modsTab = page.locator('[role="tab"]:has-text("模组"), [role="tab"]:has-text("Mods"), button:has-text("模组"), button:has-text("Mods")').first();
    await modsTab.click({ timeout: 10000 });
    await expect(page.getByText(/该实例还没有安装任何模组|No mods installed/)).toBeVisible({ timeout: 10000 });
    const downloadMoreBtn = page.locator('button:has-text("下载更多模组"), button:has-text("Download More Mods")').first();
    await downloadMoreBtn.click();
    
    // Ensure Modrinth source is selected
    const modrinthBtn = page.getByRole('button', { name: 'Modrinth' }).first();
    if (!await modrinthBtn.isVisible({ timeout: 2000 }).catch(() => false)) {
      const sourceBtn = page.getByRole('button', { name: 'CurseForge' }).first();
      await sourceBtn.click();
      await page.locator('[role="option"]:has-text("Modrinth")').first().click();
    }
    
    await page.getByRole('textbox', { name: /搜索|Search/ }).fill('JEI');
    await page.getByRole('button', { name: /搜索|Search/ }).click();
    await page.getByRole('heading', { name: 'Just Enough Items (JEI)' }).click();
    const installBtn = page.getByRole('button', { name: /安装到选中实例|Install to Selected Instance/ });
    await expect(installBtn).toBeVisible({ timeout: 60000 });

    // Uncheck dependencies for JEI so it installs as an independent mod
    const depCheckbox = page.getByRole('checkbox', { name: /同时下载安装检测到的前置依赖|Download.*dependencies/ });
    if (await depCheckbox.isVisible({ timeout: 3000 }).catch(() => false)) {
      if (await depCheckbox.isChecked()) {
        await depCheckbox.uncheck();
      }
    }

    await installBtn.click();
    await expect(installBtn).not.toBeVisible({ timeout: 10000 });

    // Wait for mod JAR to appear in instance directory (allowing up to 180s for network downloads on slow connections)
    await expect(async () => {
      const files = fs.existsSync(modsDir) ? fs.readdirSync(modsDir).filter(f => f.endsWith('.jar')) : [];
      expect(files.length).toBeGreaterThanOrEqual(1);
    }).toPass({ timeout: 180000 });
  });

  test('3. Install dependent mod: AppleSkin', async ({ page }, testInfo) => {
    const safeWorkerIndex = parseInt(String(testInfo.workerIndex), 10);
    const modsDir = resolveSafePath(process.cwd(), 'e2e', '.temp', `worker-${safeWorkerIndex}`, '.minecraft', 'versions', 'E2E_Fabric_Test', 'mods');

    await page.getByRole('textbox', { name: /搜索|Search/ }).fill('AppleSkin');
    await page.getByRole('button', { name: /搜索|Search/ }).click();
    await page.getByRole('heading', { name: 'AppleSkin' }).click();
    await expect(page.getByText('Fabric API').first()).toBeVisible({ timeout: 60000 });
    await expect(page.getByRole('checkbox', { name: /同时下载安装检测到的前置依赖|Download.*dependencies/ })).toBeEnabled({ timeout: 60000 });
    const installBtn = page.getByRole('button', { name: /安装到选中实例|Install to Selected Instance/ });
    await installBtn.click();
    await expect(installBtn).not.toBeVisible({ timeout: 10000 });

    // Wait for all 3 mod JARs (JEI, AppleSkin, Fabric API) to appear in instance directory
    await expect(async () => {
      const files = fs.existsSync(modsDir) ? fs.readdirSync(modsDir).filter(f => f.endsWith('.jar')) : [];
      expect(files.length).toBeGreaterThanOrEqual(3);
    }).toPass({ timeout: 180000 });
  });

  test('4. Verify both mods are installed successfully', async ({ page }) => {
    const instancesNav = page.locator('a[href="/instances"]').first();
    await instancesNav.click({ timeout: 10000 });
    await page.locator('text=E2E_Fabric_Test').first().click();
    const modsTab = page.locator('[role="tab"]:has-text("模组"), [role="tab"]:has-text("Mods"), button:has-text("模组"), button:has-text("Mods")').first();
    await modsTab.click({ timeout: 10000 });
    await expect(page.getByText('AppleSkin', { exact: true })).toBeVisible({ timeout: 10000 });
    await expect(page.getByText(/MezzConfig|Fabric API/)).toBeVisible({ timeout: 10000 });
    await expect(page.getByText('Just Enough Items', { exact: true })).toBeVisible({ timeout: 10000 });
  });

  test('5. Local mod search, toggle disable/enable, and delete', async ({ page }) => {
    const instancesNav = page.locator('a[href="/instances"]').first();
    await instancesNav.click({ timeout: 10000 });
    await page.locator('text=E2E_Fabric_Test').first().click();
    const modsTab = page.locator('[role="tab"]:has-text("模组"), [role="tab"]:has-text("Mods"), button:has-text("模组"), button:has-text("Mods")').first();
    await modsTab.click({ timeout: 10000 });

    // 1. Search filter
    const searchInput = page.getByPlaceholder(/搜索已安装模组|Search/);
    await expect(searchInput).toBeVisible({ timeout: 10000 });
    await searchInput.fill('AppleSkin');
    await expect(page.getByText('AppleSkin', { exact: true })).toBeVisible();
    await expect(page.getByText('Just Enough Items', { exact: true })).not.toBeVisible();
    await searchInput.clear();
    await expect(page.getByText('Just Enough Items', { exact: true })).toBeVisible();

    // 2. Toggle disable
    const appleCard = page.locator('div.border', { hasText: 'AppleSkin' }).first();
    const toggleBtn = appleCard.locator('button:has-text("禁用"), button:has-text("Disable")').first();
    await expect(toggleBtn).toBeVisible({ timeout: 10000 });
    await toggleBtn.click();
    await expect(appleCard.locator('button:has-text("启用"), button:has-text("Enable")').first()).toBeVisible({ timeout: 5000 });

    // Toggle back to enable
    const enableBtn = appleCard.locator('button:has-text("启用"), button:has-text("Enable")').first();
    await expect(enableBtn).toBeVisible({ timeout: 5000 });
    await enableBtn.click();
    await expect(appleCard.locator('button:has-text("禁用"), button:has-text("Disable")').first()).toBeVisible({ timeout: 5000 });

    // 3. Delete mod
    const deleteBtn = appleCard.locator('button:has(.lucide-trash-2)').first();
    await expect(deleteBtn).toBeVisible({ timeout: 10000 });
    await deleteBtn.click();

    // Confirm delete in AlertDialog
    await expect(page.getByText(/确定要删除此模组吗|Are you sure/)).toBeVisible();
    const confirmDeleteBtn = page.getByRole('button', { name: /删除|Delete/ }).last();
    await confirmDeleteBtn.click();

    // Verify AppleSkin is removed
    await expect(page.getByText('AppleSkin', { exact: true })).not.toBeVisible({ timeout: 5000 });
  });

});
