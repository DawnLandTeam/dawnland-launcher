import type { LogLevelSetting } from './log';
import type { BackgroundSettings } from './background';

export type AssetCategory =
  | 'Mod'
  | 'ResourcePack'
  | 'ShaderPack'
  | 'Datapack'
  | 'Save';

export type FilenameFormatMode = 'auto' | 'preset' | 'custom';

export type FilenamePreset =
  | 'bracketPrefix'
  | 'cnBracketPrefix'
  | 'dashPrefix'
  | 'dashSuffix'
  | 'zhVersion'
  | 'original';

export type ManagementDisplayStyle = 'translationFirst' | 'filenameFirst';

export interface ResourceNamingSettings {
  filenameMode: FilenameFormatMode;
  filenamePreset: FilenamePreset;
  customTemplate: string;
  renameTargets: AssetCategory[];
  managementDisplayStyle: ManagementDisplayStyle;
  enableOnlineTranslation: boolean;
  autoSyncTranslations: boolean;
}

export interface LauncherAiConfig {
  providerType: 'remoteApi' | 'embeddedLlm';
  remoteApiKey?: string;
  remoteBaseUrl?: string;
  remoteModel?: string;
  activeEmbeddedModel?: string;
  maxRamUsage?: number;
  unlockContextSize?: boolean;
}

export type CacheRetentionPolicy = 'immediate' | '24h' | '7d';

export interface CacheGcReport {
  freedBytes: number;
  removedPoolObjects: number;
  removedTmpFiles: number;
  removedModCacheEntries: number;
  removedModIcons: number;
  removedVersionCaches: number;
  removedAssets: number;
  removedLibraries: number;
  skippedReason?: string | null;
}

export interface LauncherSettings {
  enableInstanceInheritance: boolean;
  downloadSource: 'official' | 'bmclapi';
  maxConcurrentDownloads: number;
  enableTelemetry: boolean | null;
  globalMaxMemory?: number | null;
  aiConfig?: LauncherAiConfig;
  enableGlobalGameSettings: boolean;
  enableGlobalModCache: boolean;
  autoCleanCache?: boolean;
  cacheRetentionPolicy?: CacheRetentionPolicy;
  autoCleanSharedAssets?: boolean;
  enableJvmTuner: boolean;
  uiLanguage?: string | null;
  resourceNaming?: ResourceNamingSettings;
  logLevel?: LogLevelSetting | null;
  logRetentionDays?: number;
  background?: BackgroundSettings;
}

export interface TemplatePreviewResult {
  rendered: string;
  fallbackRendered: string;
  warnings: string[];
}

export interface TranslationSearchCandidate {
  category: string;
  cfProjectId?: string | null;
  cfSlug?: string | null;
  mrProjectId?: string | null;
  mrSlug?: string | null;
  rawName: string;
  zhName: string;
  zhShortName: string;
  wikiId?: number | null;
}

export interface TranslationDbStats {
  totalEntries: number;
  verifiedEntries: number;
  userEntries: number;
  aiEntries: number;
  snapshotVersion: string;
  lastSyncAt: number;
}

export interface TranslationLookupResult {
  zhName: string;
  zhShortName: string;
  zhSummary?: string | null;
  wikiId?: number | null;
  trustLevel: number;
  sourceTag: string;
}

export interface UnifiedModProject {
  source: 'modrinth' | 'curseforge' | string;
  project_id: string;
  slug: string;
  title: string;
  description: string;
  icon_url?: string | null;
  downloads: number;
  author: string;
  mc_versions: string[];
  loaders: string[];
  download_url?: string | null;
  file_id?: string | null;
}

export interface EnrichedModProject extends UnifiedModProject {
  zhTitle?: string | null;
  zhSummary?: string | null;
  wikiId?: number | null;
  trustLevel?: number | null;
}

export interface LocalModItem {
  filename: string;
  enabled: boolean;
  size: number;
  modId?: string | null;
  name?: string | null;
  version?: string | null;
  iconUrl?: string | null;
  managedByModpack: boolean;
  provides: string[];
  originalFilename?: string | null;
  zhName?: string | null;
  zhSummary?: string | null;
  source?: string | null;
  projectId?: string | null;
  slug?: string | null;
  wikiId?: number | null;
  trustLevel?: number | null;
}

export interface LocalAssetItem {
  filename: string;
  isDir: boolean;
  size: number;
  managedByModpack: boolean;
  originalFilename?: string | null;
  zhName?: string | null;
  zhSummary?: string | null;
  source?: string | null;
  projectId?: string | null;
  slug?: string | null;
  wikiId?: number | null;
  trustLevel?: number | null;
}

export interface LocalDatapackItem {
  filename: string;
  is_dir: boolean;
  size: number;
  managed_by_modpack: boolean;
  original_filename?: string | null;
  zh_name?: string | null;
  zh_summary?: string | null;
  source?: string | null;
  project_id?: string | null;
  slug?: string | null;
  wiki_id?: number | null;
  trust_level?: number | null;
}

export interface RenamePreviewItem {
  category: AssetCategory;
  currentRelPath: string;
  targetRelPath: string;
  currentFilename: string;
  targetFilename: string;
  originalFilename: string;
  zhName?: string | null;
  willChange: boolean;
  reason: string;
}

export interface RenameApplyResult {
  renamedCount: number;
  skippedCount: number;
  failed: string[];
}

export const RESOURCE_NAMING_UPDATED_EVENT = 'dawnland:resource-naming-updated';
