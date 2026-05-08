export type RosettaDocumentFormat =
  | "txt"
  | "markdown"
  | "docx"
  | "pdf"
  | "epub"
  | "html";

export type RosettaBlockType =
  | "heading"
  | "paragraph"
  | "list_item"
  | "table_cell"
  | "blockquote"
  | "code"
  | "caption"
  | "footnote"
  | "metadata";

export type SegmentStatus =
  | "pending"
  | "translating"
  | "done"
  | "failed"
  | "skipped"
  | "edited";

export type JobStatus =
  | "created"
  | "parsing"
  | "ready"
  | "translating"
  | "paused"
  | "completed"
  | "failed"
  | "cancelled";

export type TranslationMode = "fast" | "balanced" | "coherent";

export type AppThemeMode = "light" | "dark" | "system";

export type RosettaDocument = {
  id: string;
  filename: string;
  format: RosettaDocumentFormat;
  sourceLang?: string;
  targetLang: string;
  blocks: RosettaBlock[];
};

export type RosettaBlock = {
  id: string;
  type: RosettaBlockType;
  sourceText: string;
  translatedText?: string;
  shouldTranslate: boolean;
  order: number;
  path?: string;
  style?: Record<string, unknown>;
  status: SegmentStatus;
};

export type Segment = {
  id: string;
  blockId: string;
  order: number;
  sourceText: string;
  translatedText?: string;
  sourceLang?: string;
  targetLang: string;
  kind: RosettaBlockType;
  preserveWhitespace: boolean;
  status: SegmentStatus;
};

export type RosettaJob = {
  id: string;
  filename: string;
  status: JobStatus;
  createdAt: string;
  updatedAt: string;
  targetLang: string;
  segmentCount: number;
  completedSegments: number;
  failedSegments: number;
};

export type RwkvConnectionConfig = {
  baseUrl: string;
  batchEndpoint: "/translate/v1/batch-translate" | "/big_batch/completions";
  timeoutMs: number;
  mode: TranslationMode;
};

export type RwkvRuntimeState =
  | "not-installed"
  | "partial"
  | "installed"
  | "invalid";

export type RwkvArtifactManifest = {
  id: string;
  version?: string;
  source?: string;
  filename?: string;
  sha256?: string;
  sizeBytes?: number;
  contextTokens?: number;
  supportedDirections?: string[];
  installedAt?: string;
};

export type RwkvRuntimeStatus = {
  state: RwkvRuntimeState;
  apiUrl: string;
  runtimeDir: string;
  modelDir: string;
  logsDir: string;
  runtimeDirExists: boolean;
  modelDirExists: boolean;
  logsDirExists: boolean;
  runtimeManifestExists: boolean;
  modelManifestExists: boolean;
  runtimeManifest?: RwkvArtifactManifest;
  modelManifest?: RwkvArtifactManifest;
  manifestError?: string;
  logFile: string;
  message: string;
};

export type RwkvRuntimeInstallItemKind = "runtime" | "model";

export type RwkvRuntimeInstallItemState = "missing" | "ready" | "invalid";

export type RwkvRuntimeInstallPlanItem = {
  id: string;
  kind: RwkvRuntimeInstallItemKind;
  state: RwkvRuntimeInstallItemState;
  label: string;
  targetDir: string;
  manifestPath: string;
  artifactPath?: string;
  message: string;
};

export type RwkvRuntimeInstallPlan = {
  ready: boolean;
  items: RwkvRuntimeInstallPlanItem[];
  message: string;
};

export type RwkvRuntimeInstallProgressState =
  | "not-started"
  | "queued"
  | "ready"
  | "blocked";

export type RwkvRuntimeInstallProgressItemState =
  | "pending"
  | "ready"
  | "blocked";

export type RwkvRuntimeInstallProgressItem = {
  id: string;
  kind: RwkvRuntimeInstallItemKind;
  state: RwkvRuntimeInstallProgressItemState;
  label: string;
  bytesTotal?: number;
  bytesDone: number;
  message: string;
};

export type RwkvRuntimeInstallProgress = {
  state: RwkvRuntimeInstallProgressState;
  items: RwkvRuntimeInstallProgressItem[];
  message: string;
};

export type RwkvRuntimeArtifactCatalogItemState = "ready";

export type RwkvRuntimeArtifactCatalogItem = {
  id: string;
  kind: RwkvRuntimeInstallItemKind;
  state: RwkvRuntimeArtifactCatalogItemState;
  label: string;
  targetDir: string;
  manifestPath: string;
  artifactFilename?: string;
  downloadUrl?: string;
  sourcePage?: string;
  sizeBytes?: number;
  sha256?: string;
  message: string;
};

export type RwkvRuntimeArtifactCatalog = {
  readyForDownload: boolean;
  items: RwkvRuntimeArtifactCatalogItem[];
  message: string;
};
