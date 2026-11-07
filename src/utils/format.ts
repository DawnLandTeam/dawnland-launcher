/**
 * Formatting utilities for file sizes, speeds, and numerical data.
 */

/**
 * Formats bytes into a human-readable size string (B, KB, MB, GB, TB).
 * @param bytes Size in bytes
 * @param decimals Number of decimal places (default: 1)
 */
export function formatBytes(bytes: number | undefined | null, decimals = 1): string {
  if (bytes === undefined || bytes === null || bytes <= 0 || isNaN(bytes)) {
    return '0 B';
  }
  const k = 1024;
  const dm = decimals < 0 ? 0 : decimals;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(k)), sizes.length - 1);
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(dm))} ${sizes[i]}`;
}

/**
 * Formats a download speed in bytes/sec into a uniform human-readable string.
 * @param speed Speed in bytes per second
 * @param decimals Number of decimal places (default: 1)
 */
export function formatSpeed(speed: number | undefined | null, decimals = 1): string {
  if (speed === undefined || speed === null || speed <= 0 || isNaN(speed)) {
    return '';
  }
  const k = 1024;
  const dm = decimals < 0 ? 0 : decimals;
  const sizes = ['B/s', 'KB/s', 'MB/s', 'GB/s'];
  const i = Math.min(Math.floor(Math.log(speed) / Math.log(k)), sizes.length - 1);
  return `${parseFloat((speed / Math.pow(k, i)).toFixed(dm))} ${sizes[i]}`;
}
