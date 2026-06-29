import { test, expect } from './fixtures';
import fs from 'fs';
import path from 'path';

//1. Install a Fabric 26.2 instance
//2. Install independent mod: JEI
//3. Install dependent mod: AppleSkin
//4. Verify both mods are installed successfully

test.describe.serial('Fabric Mod Workflow (E2E)', () => {
  
  test('1. Setup mock instance', async ({ page }, testInfo) => {
    // Explicitly parse and sanitize the worker index
    const safeWorkerIndex = parseInt(String(testInfo.workerIndex), 10);
    const e2eTempDir = path.resolve(process.cwd(), 'e2e', '.temp', `worker-${safeWorkerIndex}`);
    const mockInstanceDir = path.join(e2eTempDir, '.minecraft', 'versions', 'E2E_Fabric_Test');
    
    // Create the mock directory and configuration
    fs.mkdirSync(mockInstanceDir, { recursive: true });
    fs.writeFileSync(path.join(mockInstanceDir, 'E2E_Fabric_Test.json'), JSON.stringify({
      id: "E2E_Fabric_Test",
      inheritsFrom: "26.2",
      libraries: [{ name: "net.fabricmc:fabric-loader:0.15.0" }]
    }));
    
    // Navigate to instances page to trigger a fresh scan by the backend
    await page.getByRole('link', { name: '主页' }).click();
    await page.getByRole('link', { name: '实例', exact: true }).click();
    
    // Wait for the UI to reflect the newly seeded mock instance
    await expect(page.getByRole('heading', { name: 'E2E_Fabric_Test' })).toBeVisible({ timeout: 15000 });
  });

  test('2. Install independent mod: JEI', async ({ page }) => {
    await page.getByRole('link', { name: '实例', exact: true }).click();
    await page.getByRole('heading', { name: 'E2E_Fabric_Test' }).click();
    await page.getByRole('button', { name: '模组' }).first().click();
    await expect(page.getByText('该实例还没有安装任何模组')).toBeVisible();
    await page.getByRole('button', { name: '下载更多模组' }).click();
    await page.getByRole('textbox', { name: '搜索' }).fill('JEI');
    await page.getByRole('button', { name: '搜索' }).click();
    await page.getByRole('heading', { name: 'Just Enough Items (JEI)' }).click();
    await expect(page.getByText('所有依赖已满足或无前置依赖。')).toBeVisible();
    await page.getByRole('button', { name: '安装到选中实例' }).click();
    await expect(page.getByRole('heading', { name: '安装模组 Just Enough Items (JEI) 完成' })).toBeVisible({ timeout: 120000 });
  });

  test('3. Install dependent mod: AppleSkin', async ({ page }) => {
    await page.getByRole('textbox', { name: '搜索' }).fill('AppleSkin');
    await page.getByRole('button', { name: '搜索' }).click();
    await page.getByRole('heading', { name: 'AppleSkin' }).click();
    await expect(page.locator('div').filter({ hasText: 'Fabric API' }).nth(4)).toBeVisible({ timeout: 60000 });
    await expect(page.getByRole('checkbox', { name: '同时下载安装检测到的前置依赖' })).toBeEnabled({ timeout: 60000 });
    await page.getByRole('button', { name: '安装到选中实例' }).click();
    await expect(page.getByRole('heading', { name: '安装模组 AppleSkin 完成' })).toBeVisible({ timeout: 120000 });
  });

  test('4. Verify both mods are installed successfully', async ({ page }) => {
    await page.getByRole('link', { name: '实例', exact: true }).click();
    await page.getByRole('heading', { name: 'E2E_Fabric_Test' }).click();
    await page.getByRole('button', { name: '模组' }).first().click();
    await expect(page.getByText('AppleSkin', { exact: true })).toBeVisible();
    await expect(page.getByText('Fabric API')).toBeVisible();
    await expect(page.getByText('Just Enough Items', { exact: true })).toBeVisible();
  });

});
