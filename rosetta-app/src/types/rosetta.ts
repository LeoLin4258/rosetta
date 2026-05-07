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
