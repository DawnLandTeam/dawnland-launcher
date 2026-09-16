import STEVE_SKIN_BASE64 from '../assets/steve.png';
import ALEX_SKIN_BASE64 from '../assets/alex.png';

export { STEVE_SKIN_BASE64, ALEX_SKIN_BASE64 };

const imageBase64Cache = new Map<string, string>();

export async function getProxiedImageBase64(invoke: any, url: string, force: boolean = false): Promise<string> {
  if (!force && imageBase64Cache.has(url)) {
    return imageBase64Cache.get(url)!;
  }
  const result = await invoke('proxy_image_base64', { url });
  imageBase64Cache.set(url, result as string);
  return result as string;
}
