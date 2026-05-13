export type RosettaDocumentFormat =
  | "txt"
  | "markdown"
  | "docx"
  | "pdf"
  | "epub"
  | "html";

export type RosettaSourceDocumentFormat = Extract<
  RosettaDocumentFormat,
  "txt" | "markdown" | "pdf"
>;

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

export type SourceFileTranslationStatus =
  | "untranslated"
  | "translating"
  | "translated"
  | "failed";

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
  schemaVersion: number;
  id: string;
  filename: string;
  format: RosettaSourceDocumentFormat;
  sourceLang?: string | null;
  targetLang: string;
  files: RosettaSourceFile[];
  blocks: RosettaBlock[];
};

export type RosettaSourceFile = {
  id: string;
  filename: string;
  relativePath: string;
  format: RosettaSourceDocumentFormat;
  sourceLang?: string | null;
  targetLang?: string | null;
  translationStatus?: SourceFileTranslationStatus;
  segmentCount?: number;
  completedSegments?: number;
  failedSegments?: number;
  translatingSegments?: number;
  blockIds: string[];
};

export type RosettaBlock = {
  id: string;
  fileId?: string | null;
  type: RosettaBlockType;
  sourceText: string;
  translatedText?: string | null;
  shouldTranslate: boolean;
  order: number;
  path?: string | null;
  style?: Record<string, unknown> | null;
  status: SegmentStatus;
};

export type Segment = {
  id: string;
  blockId: string;
  fileId?: string | null;
  order: number;
  sourceText: string;
  translatedText?: string | null;
  sourceLang?: string | null;
  targetLang: string;
  kind: RosettaBlockType;
  preserveWhitespace: boolean;
  status: SegmentStatus;
  blockOrder?: number | null;
  segmentIndexInBlock?: number | null;
  error?: string | null;
  translationHistory?: TranslationHistoryEntry[] | null;
};

export type TranslationHistoryEntry = {
  id: string;
  runId?: string | null;
  translatedText: string;
  createdAt: string;
  sourceLang?: string | null;
  targetLang: string;
  reason: "retranslation" | "language-change";
};

export type RosettaJob = {
  schemaVersion: number;
  id: string;
  filename: string;
  format: RosettaSourceDocumentFormat;
  sourcePath?: string | null;
  sourceFilename: string;
  sourceKind: "file" | "directory";
  fileCount: number;
  sourceFiles: RosettaSourceFile[];
  status: JobStatus;
  createdAt: string;
  updatedAt: string;
  exportedAt?: string | null;
  lastError?: string | null;
  targetLang: string;
  segmentCount: number;
  completedSegments: number;
  failedSegments: number;
};

export type RosettaJobSummary = RosettaJob;

export type RosettaJobBundle = {
  schemaVersion: number;
  job: RosettaJobSummary;
  document: RosettaDocument;
  segments: Segment[];
  translationFiles: RosettaTranslationFile[];
  translationRevisions: TranslationRevision[];
};

export type RosettaTranslationFile = {
  id: string;
  sourceFileId: string;
  targetLang: string;
  status: SourceFileTranslationStatus;
  segmentCount: number;
  completedSegments: number;
  failedSegments: number;
  updatedAt: string;
  exportedAt?: string | null;
};

export type TranslationSegment = {
  sourceSegmentId: string;
  translatedText?: string | null;
  targetLang: string;
  status: SegmentStatus;
  error?: string | null;
  translationHistory?: TranslationHistoryEntry[] | null;
};

export type RosettaTranslationFileBundle = {
  translationFile: RosettaTranslationFile;
  segments: TranslationSegment[];
};

export type RosettaExportKind = "translation" | "bilingual";

export type TranslationRevisionReason =
  | "file-retranslation"
  | "selection-retranslation"
  | "language-change";

export type TranslationRevision = {
  id: string;
  jobId: string;
  fileId: string;
  createdAt: string;
  sourceLang?: string | null;
  targetLang: string;
  reason: TranslationRevisionReason;
  scopeBlockIds?: string[] | null;
  segmentTranslations: Record<string, string>;
};

export type ActiveTranslationRun = {
  id: string;
  jobId: string;
  sourceFileId: string;
  translationFileId: string;
  scope: "file" | "selection" | "retry-failed" | "batch";
  targetSegmentIds: string[];
  completedSegmentIds: string[];
  failedSegmentIds: string[];
  startedAt: string;
};

export type RosettaExportResult = {
  job: RosettaJobSummary;
  targetPath: string;
  kind: RosettaExportKind;
  bytesWritten: number;
  filesWritten: number;
  message: string;
};

export type RosettaJobFileDeleteResult = {
  deletedJob: boolean;
  jobs: RosettaJobSummary[];
  bundle?: RosettaJobBundle | null;
  message: string;
};

export type RwkvConnectionConfig = {
  baseUrl: string;
  endpoint: string;
  internalToken: string;
  bodyPassword: string;
  timeoutMs: number;
  mode: TranslationMode;
};

export type RwkvProviderId =
  | "rwkv-lightning-contents"
  | "rwkv-mobile-batch-chat"
  | "custom-rwkv-api";

export type RwkvLightningContentsProviderHandle = {
  id: "rwkv-lightning-contents";
  baseUrl: string;
  endpoint: string;
  internalToken: string;
  bodyPassword: string;
  timeoutMs: number;
};

export type RwkvMobileBatchChatProviderHandle = {
  id: "rwkv-mobile-batch-chat";
  baseUrl: string;
  timeoutMs: number;
};

export type RwkvProviderHandle =
  | RwkvLightningContentsProviderHandle
  | RwkvMobileBatchChatProviderHandle;

export type RwkvMobileBatchChatProbeRequest = {
  baseUrl: string;
  timeoutMs: number;
  sourceLang?: string | null;
  targetLang?: string | null;
};

export type RwkvMobileBatchChatTranslateRequest = {
  baseUrl: string;
  timeoutMs: number;
  sourceLang?: string | null;
  targetLang?: string | null;
  sourceTexts: string[];
};

export type RwkvMobileBatchChatRunStartRequest = {
  runId: string;
  jobId: string;
  translationFileId: string;
  sourceSegmentIds: string[];
  baseUrl: string;
  timeoutMs: number;
  sourceLang?: string | null;
  targetLang: string;
  batchSize: number;
};

export type RwkvTranslationApiProbeRequest = {
  baseUrl: string;
  endpoint: string;
  internalToken: string;
  bodyPassword: string;
  timeoutMs: number;
};

export type RwkvTranslationApiProbeResult = {
  ok: boolean;
  statusCode: number | null;
  translations: string[];
  rawResponsePreview: string;
  message: string;
  latencyMs: number;
};

export type RwkvTranslationApiTranslateRequest = {
  baseUrl: string;
  endpoint: string;
  internalToken: string;
  bodyPassword: string;
  timeoutMs: number;
  sourceLang?: string | null;
  targetLang?: string | null;
  sourceTexts: string[];
};

export type RwkvTranslationApiTranslateResult = {
  ok: boolean;
  statusCode: number | null;
  translations: string[];
  rawResponsePreview: string;
  message: string;
  latencyMs: number;
};

export type RwkvTranslationRunState =
  | "running"
  | "cancelling"
  | "completed"
  | "failed"
  | "cancelled";

export type RwkvTranslationRunStartRequest = {
  runId: string;
  jobId: string;
  translationFileId: string;
  sourceSegmentIds: string[];
  baseUrl: string;
  endpoint: string;
  internalToken: string;
  bodyPassword: string;
  timeoutMs: number;
  sourceLang?: string | null;
  targetLang: string;
  batchSize: number;
};

export type RwkvTranslationRunStatus = {
  runId: string;
  jobId: string;
  translationFileId: string;
  state: RwkvTranslationRunState;
  completedSegmentIds: string[];
  failedSegmentIds: string[];
  message: string;
  translationFile?: RosettaTranslationFile | null;
  segments?: TranslationSegment[] | null;
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
  compatibility: RwkvRuntimeCompatibility;
  runtimeDir: string;
  modelDir: string;
  logsDir: string;
  runtimeDirExists: boolean;
  modelDirExists: boolean;
  logsDirExists: boolean;
  runtimeBundleDir: string;
  runtimeBundleExists: boolean;
  runtimeExecutablePath: string;
  runtimeExecutableExists: boolean;
  runtimeManifestExists: boolean;
  modelManifestExists: boolean;
  runtimeManifest?: RwkvArtifactManifest;
  modelManifest?: RwkvArtifactManifest;
  manifestError?: string;
  logFile: string;
  message: string;
};

export type RwkvRuntimeCompatibility = {
  runtimeBackend: string;
  hardwareRequirement: string;
  detectedDisplayAdapters: string[];
  compatible: boolean;
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

export type RwkvRuntimeArtifactScanResult = {
  scanned: boolean;
  installedManifests: string[];
  errors: string[];
  plan: RwkvRuntimeInstallPlan;
  message: string;
};

export type RwkvRuntimeExtractionResult = {
  extracted: boolean;
  targetDir: string;
  executablePath: string;
  filesExtracted: number;
  bytesExtracted: number;
  plan: RwkvRuntimeInstallPlan;
  message: string;
};

export type RwkvRuntimeProcessState = "stopped" | "starting" | "ready";

export type RwkvRuntimeProcessStatus = {
  state: RwkvRuntimeProcessState;
  pid: number | null;
  processRunning: boolean | null;
  pidFile: string;
  apiUrl: string;
  port: number;
  portOpen: boolean;
  httpReady: boolean;
  httpStatusCode: number | null;
  logFile: string;
  logTail: string[];
  message: string;
};

export type RwkvRuntimeStartResult = {
  started: boolean;
  command: string[];
  process: RwkvRuntimeProcessStatus;
  message: string;
};

export type RwkvRuntimeTranslationProbeResult = {
  ok: boolean;
  statusCode: number | null;
  responseBodyPreview: string;
  process: RwkvRuntimeProcessStatus;
  message: string;
};

// -----------------------------------------------------------------------------
// Managed local RWKV runtime (Phase 3 — macOS-first per ADR 0003).
//
// These types mirror `src-tauri/src/managed_rwkv/` and are deliberately
// separate from the legacy `RwkvRuntime*` types above. The legacy ones stay
// as "paused" placeholders until the Windows path resumes (Phase 8); the new
// names below are what `selectProvider()` and the Settings UI (Phase 5) read.
// -----------------------------------------------------------------------------

export type ManagedRuntimeState =
  | "unsupported"
  | "not-installed"
  | "installed"
  | "starting"
  | "ready"
  | "failed"
  | "stopped";

export type ManagedRuntimeInstallItemKind = "sidecar" | "tokenizer" | "model";

export type ManagedRuntimeInstallItemState = "missing" | "present";

export type ManagedRuntimeInstallItem = {
  kind: ManagedRuntimeInstallItemKind;
  state: ManagedRuntimeInstallItemState;
  path: string;
  sizeBytes: number | null;
  message: string;
};

export type ManagedRuntimeInstallPlan = {
  ready: boolean;
  items: ManagedRuntimeInstallItem[];
  message: string;
};

export type ManagedRuntimeProfileSummary = {
  id: string;
  providerId: string;
  platformOs: string;
  platformArch: string;
  backend: string;
  modelFilename: string;
  modelSizeBytes: number;
  modelSha256: string;
  supportedDirections: string[];
  bindHost: string;
};

export type ManagedRuntimePaths = {
  sidecar: string | null;
  tokenizer: string | null;
  modelFile: string;
  logsDir: string;
};

export type ManagedRuntimeProcessSnapshot = {
  pid: number | null;
  port: number | null;
  baseUrl: string | null;
  startedAt: string | null;
  lastError: string | null;
};

export type ManagedRuntimeStatus = {
  state: ManagedRuntimeState;
  message: string;
  profile: ManagedRuntimeProfileSummary | null;
  paths: ManagedRuntimePaths | null;
  installPlan: ManagedRuntimeInstallPlan | null;
  process: ManagedRuntimeProcessSnapshot;
};

export type ManagedRuntimeStartResult = {
  pid: number;
  port: number;
  baseUrl: string;
  startedAt: string;
  command: string[];
  message: string;
};

export type ManagedRuntimeProbeResult = {
  ok: boolean;
  statusCode: number | null;
  latencyMs: number;
  baseUrl: string | null;
  message: string;
};

export type ManagedRuntimeLogsSummary = {
  logFile: string;
  logTail: string[];
  message: string;
};

// -----------------------------------------------------------------------------
// Managed-runtime install (Phase 4) — model download progress + result.
// -----------------------------------------------------------------------------

export type ManagedRuntimeInstallPhase =
  | "idle"
  | "preflight"
  | "downloading"
  | "verifying"
  | "writing-manifest"
  | "done"
  | "failed"
  | "cancelled";

export type ManagedRuntimeInstallProgress = {
  phase: ManagedRuntimeInstallPhase;
  bytesDone: number;
  bytesTotal: number;
  sourceUrl: string | null;
  speedBytesPerSec: number;
  startedAt: string | null;
  message: string;
  lastError: string | null;
};

/**
 * Options accepted by `install_managed_rwkv_runtime`. The Tauri command takes
 * an optional `options` argument; pass `{ repair: true }` to wipe any existing
 * `.part` / `.part.broken` / model files before retrying.
 */
export type ManagedRuntimeInstallOptions = {
  repair?: boolean;
};

/**
 * Final outcome of an install. Returned when the command resolves; UIs that
 * want live progress should also subscribe to the
 * `managed-rwkv://install-progress` event.
 */
export type ManagedRuntimeInstallResult = {
  ready: boolean;
  installed: boolean;
  phase: ManagedRuntimeInstallPhase;
  bytesDone: number;
  bytesTotal: number;
  sourceUrl: string | null;
  message: string;
  manifestPath: string;
};

export type ManagedRuntimeCancelInstallResult = {
  cancelled: boolean;
  message: string;
};
