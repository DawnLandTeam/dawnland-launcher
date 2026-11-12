const WINDOWS_RESERVED_NAMES = new Set([
  'CON',
  'PRN',
  'AUX',
  'NUL',
  'CONIN$',
  'CONOUT$',
  'COM0',
  'COM1',
  'COM2',
  'COM3',
  'COM4',
  'COM5',
  'COM6',
  'COM7',
  'COM8',
  'COM9',
  'COM\u00b9',
  'COM\u00b2',
  'COM\u00b3',
  'LPT0',
  'LPT1',
  'LPT2',
  'LPT3',
  'LPT4',
  'LPT5',
  'LPT6',
  'LPT7',
  'LPT8',
  'LPT9',
  'LPT\u00b9',
  'LPT\u00b2',
  'LPT\u00b3',
]);

/**
 * Validates whether a proposed instance name is safe as a single directory name
 * across Windows, macOS, and Linux, matching backend `fs_utils::is_safe_single_name`.
 */
export function isSafeInstanceName(name: string): boolean {
  if (!name) return false;
  if (name.endsWith('.') || name.endsWith(' ') || name.trimEnd() !== name) {
    return false;
  }
  if (/[<>:"/\\|?*\x00-\x1F]/.test(name)) {
    return false;
  }
  if (name.split('').every((c) => c === '.')) {
    return false;
  }
  const stem = (name.split('.')[0] ?? name).replace(/[. ]+$/g, '').toUpperCase();
  return !WINDOWS_RESERVED_NAMES.has(stem);
}
