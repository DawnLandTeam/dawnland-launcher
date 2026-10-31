export type DetectedFileKind =
  | 'Modpack'
  | 'Mod'
  | 'ResourcePack'
  | 'ShaderPack'
  | 'Datapack'
  | 'World'
  | 'Unknown';

export interface InspectedDropFile {
  path: string;
  filename: string;
  kind: DetectedFileKind;
  displayName?: string | null;
  detectedLoader?: string | null;
  detectedMcVersion?: string | null;
  size: number;
  isDir: boolean;
}

export interface DropImportItem {
  path: string;
  kindOverride?: DetectedFileKind | null;
}

export interface ImportedFileResult {
  filename: string;
  kind: DetectedFileKind;
  targetRelPath: string;
}

export interface SkippedFileResult {
  filename: string;
  kind: DetectedFileKind;
  reason: string;
}

export interface FailedFileResult {
  filename: string;
  error: string;
}

export interface DropImportReport {
  succeeded: ImportedFileResult[];
  skipped: SkippedFileResult[];
  failed: FailedFileResult[];
}

export interface InstanceAssetsUpdatedDetail {
  instanceId: string;
  kinds: DetectedFileKind[];
}
