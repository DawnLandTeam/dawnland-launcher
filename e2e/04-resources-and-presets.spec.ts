import { test, expect } from './fixtures';
import fs from 'fs';
import path from 'path';

test.describe.serial('Resources and Presets Workflow (E2E)', () => {
  
  test('1. Setup mock neoforge instance', async ({ page }, testInfo) => {
    const safeWorkerIndex = parseInt(String(testInfo.workerIndex), 10);
    const e2eTempDir = path.resolve(process.cwd(), 'e2e', '.temp', `worker-${safeWorkerIndex}`);
    const mockInstanceDir = path.join(e2eTempDir, '.minecraft', 'versions', 'E2E_NeoForge_Preset');
    
    fs.mkdirSync(mockInstanceDir, { recursive: true });
    fs.writeFileSync(path.join(mockInstanceDir, 'E2E_NeoForge_Preset.json'), JSON.stringify({
      id: "E2E_NeoForge_Preset",
      inheritsFrom: "1.21.1",
      libraries: [{ name: "net.neoforged:neoforge:21.1.0-beta" }]
    }));
    
    await page.getByRole('link', { name: '主页' }).click();
    await page.getByRole('link', { name: '实例', exact: true }).click();
    
    await expect(page.getByRole('heading', { name: 'E2E_NeoForge_Preset' })).toBeVisible({ timeout: 15000 });
  });

  test('2. Download Resourcepack from Modrinth', async ({ page }) => {
    await page.getByRole('link', { name: '实例', exact: true }).click();
    await page.getByRole('heading', { name: 'E2E_NeoForge_Preset' }).click();

    await page.getByRole('button', { name: '资源包' }).first().click();
    await expect(page.getByText('该实例还没有安装任何资源包')).toBeVisible();
    await page.getByRole('button', { name: '下载更多' }).click();

    // Ensure Modrinth is selected
    await page.getByRole('button', { name: 'CurseForge' }).click();
    await page.locator('div').filter({ hasText: /^Modrinth$/ }).click();
    await page.waitForTimeout(3000)
    // Search for Bare Bones
    await page.getByRole('textbox', { name: '搜索' }).fill('Bare Bones');
    await page.getByRole('button', { name: '搜索' }).click();
    await page.getByRole('heading', { name: 'Bare Bones' }).first().click();

    await expect(page.getByRole('button', { name: '安装到选中实例' })).toBeVisible({ timeout: 60000 });
    await page.getByRole('button', { name: '安装到选中实例' }).click();

    await expect(page.getByRole('heading', { name: /完成/ })).toBeVisible({ timeout: 180000 });
  });

  test('3. Add Mod to Preset from Modrinth', async ({ page }) => {
    await page.getByRole('link', { name: '实例', exact: true }).click();
    await page.getByRole('heading', { name: 'E2E_NeoForge_Preset' }).click();
    
    // Go to Mods and Download more
    await page.getByRole('button', { name: '模组' }).first().click();
    await page.getByRole('button', { name: '下载更多模组' }).click();
    
    // Ensure Modrinth is selected
    await page.getByRole('button', { name: 'CurseForge' }).click();
    await page.locator('div').filter({ hasText: /^Modrinth$/ }).click();
    
    await page.getByPlaceholder('搜索...').fill('JEI');
    await page.getByRole('button', { name: '搜索', exact: true }).click({ timeout: 15000 });
    await expect(page.getByText('Just Enough Items').first()).toBeVisible({ timeout: 30000 });
    await page.getByText('Just Enough Items').first().click({ timeout: 15000 });
    
    await expect(page.getByText('所有依赖已满足或无前置依赖。')).toBeVisible({ timeout: 60000 });
    
    // Click Add to Preset
    await page.getByRole('button', { name: '添加到预设' }).click();
    await page.getByRole('textbox', { name: '预设名称' }).fill('E2E_NeoForge_Preset_Mod');
    await page.getByRole('button', { name: '确认' }).click({ timeout: 30000 });
    
    // Wait for the success toast to appear
    await expect(page.getByText('已成功添加到预设')).toBeVisible({ timeout: 15000 });
    await page.getByRole('button', { name: 'Close' }).first().click();
  });

  test('4. Apply Preset to Instance', async ({ page }) => {
    await page.getByRole('link', { name: '实例', exact: true }).click();
    await page.getByRole('heading', { name: 'E2E_NeoForge_Preset' }).click();
    
    // Click on the second "模组" which is the Mod Groups tab under Presets
    await page.getByRole('button', { name: '模组' }).nth(1).click();
    
    // The asset list might be empty initially, we should wait until the preset 'E2E_NeoForge_Preset_Mod.json' is visible
    await expect(page.getByText('E2E_NeoForge_Preset_Mod.json')).toBeVisible({ timeout: 15000 });
    
    // Click Apply button
    await page.getByRole('button', { name: '应用' }).click(); // Wait, there might be multiple "应用" buttons.

    // A confirmation dialog "应用预设确认" (Apply Preset Confirmation) should pop up
    await expect(page.getByRole('heading', { name: '应用预设确认' })).toBeVisible({ timeout: 120000 });
    await page.getByRole('button', { name: '开始下载' }).click();
    
    // Apply triggers a task, wait for the task to finish
    await expect(page.getByRole('heading', { name: /完成/ })).toBeVisible({ timeout: 120000 });
  });

  test('5. Verify Installation', async ({ page }) => {
    await page.getByRole('link', { name: '实例', exact: true }).click();
    await page.getByRole('heading', { name: 'E2E_NeoForge_Preset', exact: true }).click();
    
    // Check Mods list
    await page.getByRole('button', { name: '模组' }).first().click();
    await expect(page.getByText('Just Enough Items', { exact: true })).toBeVisible();
    
    // Check Resourcepacks list
    await page.getByRole('button', { name: '资源包' }).first().click();
    await expect(page.getByText('Bare Bones', { exact: false })).toBeVisible();
  });

  test('6. Graphically Edit Preset', async ({ page }) => {
    await page.getByRole('link', { name: '实例', exact: true }).click();
    await page.getByRole('heading', { name: 'E2E_NeoForge_Preset', exact: true }).click();

    // Go to Mod Groups tab
    await page.getByRole('button', { name: '模组' }).nth(1).click();

    // Ensure preset is visible
    await expect(page.getByText('E2E_NeoForge_Preset_Mod.json')).toBeVisible({ timeout: 15000 });

    // Click Edit button
    await page.getByRole('button', { name: '编辑预设' }).first().click();

    // Wait for Dialog to appear
    await expect(page.getByRole('heading', { name: /编辑预设/ })).toBeVisible({ timeout: 10000 });

    // Ensure JEI is listed
    await expect(page.getByText('Just Enough Items (JEI)', { exact: true })).toBeVisible();

    // Click Remove button
    await page.getByRole('button', { name: '移除' }).click();

    // It should be removed immediately from DOM
    await expect(page.getByText('Just Enough Items (JEI)', { exact: true })).not.toBeVisible();

    // Close dialog
    await page.getByRole('button', { name: '取消' }).click();

    // Re-open to verify persistence
    await page.getByRole('button', { name: '编辑预设' }).first().click();
    await expect(page.getByRole('heading', { name: /编辑预设/ })).toBeVisible({ timeout: 10000 });
    await expect(page.getByText('Just Enough Items (JEI)', { exact: true })).not.toBeVisible();

    // Should display empty state
    await expect(page.getByText('该预设目前为空')).toBeVisible();

    await page.getByRole('button', { name: '取消' }).click();
  });

});
