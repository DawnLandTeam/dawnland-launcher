export type BackgroundMode = 'default' | 'custom';

export type BackgroundFit = 'cover' | 'contain' | 'fill';

export const DEFAULT_BACKGROUND_BLUR_PX = 4;
export const MAX_BACKGROUND_BLUR_PX = 24;
export const MAX_BACKGROUND_OVERLAY_OPACITY = 85;
export const DEFAULT_LIGHT_OVERLAY_OPACITY = 30;
export const DEFAULT_DARK_OVERLAY_OPACITY = 60;
export const DEFAULT_BACKGROUND_SRC = '/minecraft_bg.png';

export interface BackgroundSettings {
  mode: BackgroundMode;
  imageFile: string | null;
  blurPx: number;
  overlayOpacity: number | null;
  fit: BackgroundFit;
  updatedAt: number;
}

export interface LauncherBackgroundState {
  mode: BackgroundMode;
  imageFile: string | null;
  imagePath: string | null;
  fileSizeBytes: number | null;
  blurPx: number;
  overlayOpacity: number | null;
  fit: BackgroundFit;
  updatedAt: number;
}

export interface UpdateBackgroundStylePayload {
  blurPx: number;
  overlayOpacity: number | null;
  fit: BackgroundFit;
}
