import { test, expect } from './fixtures';

test.describe('Servers View', () => {
  test('should render server list from backend API', async ({ page, mockTauri }) => {
    // Mock the backend API call for servers
    await mockTauri.setMockResponses({
      'get_servers': {
        data: [
        { 
          id: 1, 
          name: 'Dawnland Official', 
          ip: 'mc.dawnland.net',
          port: 25565,
          motd: 'The official survival server',
          version: '1.20.1',
          loaderType: 'vanilla',
          serverType: 'vanilla',
          authType: 'online',
          iconUrl: '',
          email: '',
          isActive: true
        },
        { 
          id: 2, 
          name: 'Creative Hub', 
          ip: 'creative.dawnland.net', 
          port: 25565,
          motd: 'Build whatever you want',
          version: '1.20.1',
          loaderType: 'vanilla',
          serverType: 'vanilla',
          authType: 'online',
          iconUrl: '',
          email: '',
          isActive: true
        }
        ],
        totalPages: 1
      },
      'ping_server': {
        onlinePlayers: 42,
        maxPlayers: 100,
        ping: 35
      },
      'get_filter_options': {
        versions: ['1.20.1'],
        serverTypes: ['vanilla'],
        authTypes: ['online']
      },
      'get_vanilla_versions': [],
      'scan_installed_instances': [],
      'get_accounts': []
    });

    await page.goto('/servers');

    // Wait for the servers to render
    await expect(page.getByText('Dawnland Official')).toBeVisible();
    await expect(page.getByText('Creative Hub')).toBeVisible();

    // Check player counts
    await expect(page.getByText('42 / 100').first()).toBeVisible();

  });
});
