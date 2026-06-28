import { test, expect } from './fixtures';

//1. Install a Fabric 26.2 instance
//2. Install independent mod: JEI
//3. Install dependent mod: AppleSkin
//4. Verify both mods are installed successfully

test.describe.serial('Fabric Mod Workflow (E2E)', () => {
  
  test('1. Install a Fabric 26.2 instance', async ({ page }) => {
    await page.getByRole('link', { name: '主页' }).click();
    await page.getByRole('link', { name: '安装实例' }).click();
    await page.getByRole('button', { name: '安装实例' }).click();
    await page.getByRole('textbox', { name: '输入版本名称进行搜索' }).click();
    await page.getByRole('textbox', { name: '输入版本名称进行搜索' }).fill('26.2');
    await page.getByRole('button', { name: '刷新' }).click();
    await page.locator('div').filter({ hasText: /^26\.2正式版$/ }).first().click();
    await page.locator('div').filter({ hasText: /^不安装$/ }).nth(2).click();
    await page.getByRole('button', { name: '下一步 →' }).click();
    await page.getByRole('textbox', { name: 'install.defaultName' }).click();
    await expect(page.getByText(/Fabric \d+\.\d+\.\d+/)).toBeVisible({ timeout: 16000 });
    await page.waitForTimeout(2000);
    await page.getByRole('textbox', { name: 'install.defaultName' }).fill('E2E_Fabric_Test');
    await page.getByRole('button', { name: '安装实例' }).click();
    await expect(page.getByRole('button', { name: '完成' })).toBeVisible({ timeout: 180000 });
    await page.getByRole('button', { name: '完成' }).click()
  });

  test('2. Install independent mod: JEI', async ({ page }) => {
    await page.getByRole('link', { name: '实例' }).click();
    await page.locator('div').filter({ hasText: /^E2E_Fabric_Test26\.2Fabric$/ }).nth(1).click();
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
    await page.getByRole('link', { name: '实例' }).click();
    await page.locator('div').filter({ hasText: /^E2E_Fabric_Test26\.2Fabric$/ }).nth(1).click();
    await page.getByRole('button', { name: '模组' }).first().click();
    await expect(page.getByText('AppleSkin', { exact: true })).toBeVisible();
    await expect(page.getByText('Fabric API')).toBeVisible();
    await expect(page.getByText('Just Enough Items', { exact: true })).toBeVisible();
  });

});
