import { describe, it, expect } from 'vitest';
import { parseDeepLinkUrl } from './deepLink';

describe('parseDeepLinkUrl', () => {
  it('should parse a valid modpack deep link without name', () => {
    const url = 'dlml://modpack/install?id=123&source=curseforge&version_id=456';
    const result = parseDeepLinkUrl(url);
    expect(result).toEqual({
      type: 'modpack',
      payload: { projectId: '123', source: 'curseforge', versionId: '456', name: '' }
    });
  });

  it('should parse a valid modpack deep link with name containing spaces', () => {
    const url = 'dlml://modpack/install?id=490660&source=curseforge&version_id=5.10.16&name=DeceasedCraft%20-%20Urban%20Zombie%20Apocalypse';
    const result = parseDeepLinkUrl(url);
    expect(result).toEqual({
      type: 'modpack',
      payload: { 
        projectId: '490660', 
        source: 'curseforge', 
        versionId: '5.10.16', 
        name: 'DeceasedCraft - Urban Zombie Apocalypse' 
      }
    });
  });

  it('should parse a valid server deep link', () => {
    const url = 'dlml://server/view?id=789';
    const result = parseDeepLinkUrl(url);
    expect(result).toEqual({
      type: 'server',
      payload: { id: '789' }
    });
  });

  it('should parse a valid authlib deep link with decoded URL', () => {
    const url = 'dlml://authlib/add?url=https%3A%2F%2Fauthlib-injector.yushi.moe';
    const result = parseDeepLinkUrl(url);
    expect(result).toEqual({
      type: 'authlib',
      payload: { url: 'https://authlib-injector.yushi.moe' }
    });
  });

  it('should return null for non-dlml protocols', () => {
    const url = 'https://example.com/modpack/install?id=123';
    expect(parseDeepLinkUrl(url)).toBeNull();
  });

  it('should return null for invalid URL string', () => {
    const url = 'not-a-url';
    expect(parseDeepLinkUrl(url)).toBeNull();
  });

  it('should return null for missing required query parameters in modpack', () => {
    const url = 'dlml://modpack/install?id=123&source=curseforge'; // missing version_id
    expect(parseDeepLinkUrl(url)).toBeNull();
  });
});
