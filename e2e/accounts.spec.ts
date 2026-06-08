import { test, expect } from './fixtures';

test.describe('Accounts View', () => {
  test('should render accounts list', async ({ page, mockTauri }) => {
    await mockTauri.setMockResponses({
      'get_accounts': [
        { id: 'acc-1', username: 'Notch', accountType: 'microsoft' },
        { id: 'acc-2', username: 'Jeb_', accountType: 'offline' }
      ]
    });

    await page.goto('/accounts');

    await expect(page.getByText('Notch')).toBeVisible();
    await expect(page.getByText('Jeb_')).toBeVisible();
    
    // Check if badges rendered (assuming they contain specific text or are next to username)
    // Microsoft / Offline badging exists in DOM
  });

  test('should add offline account and send correct IPC payload', async ({ page, mockTauri }) => {
    await mockTauri.setMockResponses({
      'get_accounts': [],
      'add_offline_account': null // Returns success
    });

    await page.goto('/accounts');

    // Click Add Account button in the header
    // The button has a Plus icon, usually right side of header
    const addAccountBtn = page.locator('button', { hasText: /add|添加/i }).first();
    await addAccountBtn.click();

    // The Add Account modal should appear. We click the "Offline" option
    // It has the WifiOff icon or text
    const offlineTab = page.locator('button').filter({ has: page.locator('.lucide-wifi-off') });
    await offlineTab.click();

    // Type the username
    // The input is a text input, we can just grab the first input in the modal or by placeholder
    const usernameInput = page.locator('input[type="text"]').first();
    await usernameInput.fill('PlaywrightUser');

    // Click the confirm Add button
    const confirmAddBtn = page.locator('button', { hasText: /add|添加/i }).last();
    await confirmAddBtn.click();

    // Wait briefly for IPC to be called
    await page.waitForTimeout(500);

    // Verify IPC call
    const calls = await mockTauri.getIpcCalls();
    const addCall = calls.find(c => c.cmd === 'add_offline_account');
    
    expect(addCall).toBeDefined();
    expect(addCall?.args).toEqual({ username: 'PlaywrightUser' });
  });
});
