import { test, expect } from './fixtures';

test.describe('Home View', () => {
  test('should render empty state when no instances exist', async ({ page, mockTauri }) => {
    await mockTauri.setMockResponses({
      'get_accounts': [{ id: 'acc1', username: 'PlayerOne', accountType: 'offline' }],
      'scan_installed_instances': [],
    });

    await page.goto('/');

    // Should see empty state message
    await expect(page.getByText(/install/i).first()).toBeVisible(); // Depending on locale, check for install button in empty state
  });

  test('should render selected instance and account, and allow launching', async ({ page, mockTauri }) => {
    await mockTauri.setMockResponses({
      'get_accounts': [
        { id: 'acc-123', username: 'TestUser', accountType: 'microsoft' }
      ],
      'scan_installed_instances': [
        { id: 'inst-456', name: 'My Modpack', mcVersion: '1.20.1', loaderType: 'fabric' }
      ],
      // Mock the launch command to just return success
      'get_instance_config': { showGameLog: true },
      'launch_instance': null
    });

    await page.goto('/');

    // Wait for data to load
    await expect(page.getByText('TestUser')).toBeVisible();
    await expect(page.getByText('My Modpack')).toBeVisible();

    // Verify "Play" button is enabled
    const playButton = page.locator('button', { hasText: /launch|play|启动/i });
    await expect(playButton).toBeEnabled();

    // Click Play
    await playButton.click();

    // Wait briefly for IPC to be called
    await page.waitForTimeout(500);

    // Verify IPC call was made
    const calls = await mockTauri.getIpcCalls();
    const launchCall = calls.find(c => c.cmd === 'launch_instance');
    expect(launchCall).toBeDefined();
    expect(launchCall?.args).toEqual({
      versionId: 'inst-456',
      accountUuid: 'acc-123'
    });
  });
});
