import { describe, it, expect, vi, beforeEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { fetchApi } from './api';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn()
}));

// Setup global fetch mock
globalThis.fetch = vi.fn();

describe('fetchApi utility', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    
    // Default mock response for invoke
    (invoke as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      timestamp: '1234567890',
      signature: 'mock_signature'
    });

    // Default mock response for fetch
    (globalThis.fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue(
      new Response('ok', { status: 200 })
    );
  });

  it('should call invoke to get signature before fetching', async () => {
    await fetchApi('https://api.example.com/test?query=1', {
      method: 'POST',
      body: JSON.stringify({ data: 'hello' })
    });

    // Verify invoke was called with correct parameters
    expect(invoke).toHaveBeenCalledWith('generate_api_signature', {
      method: 'POST',
      path: '/test?query=1',
      body: JSON.stringify({ data: 'hello' })
    });
  });

  it('should set headers correctly on the fetch request', async () => {
    await fetchApi('https://api.example.com/test');

    expect(globalThis.fetch).toHaveBeenCalledTimes(1);
    const fetchCallArgs = (globalThis.fetch as unknown as ReturnType<typeof vi.fn>).mock.calls[0];
    
    // Verify headers were attached
    const options = fetchCallArgs[1] as RequestInit;
    expect(options.headers).toBeInstanceOf(Headers);
    
    const headers = options.headers as Headers;
    expect(headers.get('X-Launcher-Time')).toBe('1234567890');
    expect(headers.get('X-Launcher-Signature')).toBe('mock_signature');
  });

  it('should propagate errors if invoke fails', async () => {
    (invoke as unknown as ReturnType<typeof vi.fn>).mockRejectedValue(new Error('Signature generation failed'));

    await expect(fetchApi('https://api.example.com')).rejects.toThrow('Signature generation failed');
    expect(globalThis.fetch).not.toHaveBeenCalled();
  });
});
