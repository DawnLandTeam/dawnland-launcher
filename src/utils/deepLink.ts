import type { DeepLinkData } from "../components/DeepLinkReceiveModal.vue";

/**
 * Parses a dlml:// deep link URL string and returns standard DeepLinkData if valid.
 * Returns null if the URL is not a recognized or valid dawnland deep link.
 */
export function parseDeepLinkUrl(urlStr: string): DeepLinkData | null {
  try {
    const url = new URL(urlStr);
    if (url.protocol !== 'dlml:') {
      return null;
    }

    // Modpack installation
    if (url.pathname === '//modpack/install' || url.host === 'modpack') {
      const projectId = url.searchParams.get('id');
      const source = url.searchParams.get('source');
      const versionId = url.searchParams.get('version_id');
      const name = url.searchParams.get('name'); // Can be null
      
      if (projectId && source && versionId) {
        return {
          type: 'modpack',
          payload: { projectId, source, versionId, name: name || '' }
        };
      }
    }
    // Authlib adding
    else if (url.pathname === '//authlib/add' || url.host === 'authlib') {
      const authUrl = url.searchParams.get('url');
      if (authUrl) {
        return {
          type: 'authlib',
          payload: { url: decodeURIComponent(authUrl) }
        };
      }
    }
    // Server viewing
    else if (url.pathname === '//server/view' || url.host === 'server') {
      const serverId = url.searchParams.get('id');
      if (serverId) {
        return {
          type: 'server',
          payload: { id: serverId }
        };
      }
    }
  } catch (err) {
    // Invalid URL format
    return null;
  }
  return null;
}
