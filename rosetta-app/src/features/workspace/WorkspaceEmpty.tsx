import { useState } from "react";
import { FilePlus2, FileText, FolderOpen, Upload } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  createBlankTxtDocument,
  importRosettaDocumentFromPath,
  importRosettaProjectFromDirectory,
  pickRosettaImportDirectory,
  pickRosettaImportPath,
} from "@/lib/rosettaJobs";
import type { RosettaJobBundle } from "@/types/rosetta";

type WorkspaceEmptyProps = {
  onImported: (bundle: RosettaJobBundle) => void;
  isDraggingOver: boolean;
};

export function WorkspaceEmpty({ onImported, isDraggingOver }: WorkspaceEmptyProps) {
  const [isImporting, setIsImporting] = useState(false);
  const [importError, setImportError] = useState<string | null>(null);
  const [newFilename, setNewFilename] = useState("");
  const canCreateFile = newFilename.trim().length > 0 && !isImporting;

  async function chooseFile() {
    setIsImporting(true);
    setImportError(null);
    try {
      const path = await pickRosettaImportPath();
      if (!path) return;
      const bundle = await importRosettaDocumentFromPath(path);
      onImported(bundle);
    } catch (err) {
      setImportError(err instanceof Error ? err.message : "无法导入这个文件。");
    } finally {
      setIsImporting(false);
    }
  }

  async function chooseFolder() {
    setIsImporting(true);
    setImportError(null);
    try {
      const path = await pickRosettaImportDirectory();
      if (!path) return;
      const bundle = await importRosettaProjectFromDirectory(path);
      onImported(bundle);
    } catch (err) {
      setImportError(err instanceof Error ? err.message : "无法导入这个文件夹。");
    } finally {
      setIsImporting(false);
    }
  }

  async function createFile() {
    if (!canCreateFile) return;
    setIsImporting(true);
    setImportError(null);
    try {
      const bundle = await createBlankTxtDocument(newFilename);
      onImported(bundle);
      setNewFilename("");
    } catch (err) {
      setImportError(err instanceof Error ? err.message : "无法创建新文件。");
    } finally {
      setIsImporting(false);
    }
  }

  return (
    <div className="flex h-full flex-col items-center justify-center gap-8 px-10">
      <div
        className={`flex w-full max-w-lg flex-col items-center gap-5 rounded-xl border-2 border-dashed px-10 py-12 text-center transition-colors ${
          isDraggingOver
            ? "border-primary bg-primary/5"
            : "border-border/50 bg-muted/10"
        }`}
      >
        <div className="flex size-14 items-center justify-center rounded-xl bg-muted/40 text-muted-foreground">
          <Upload className="size-7" strokeWidth={1.5} />
        </div>
        <div className="space-y-1.5">
          <p className="text-sm font-medium">拖入文件或文件夹</p>
          <p className="text-xs text-muted-foreground">支持 TXT · Markdown · PDF</p>
        </div>
        <div className="grid w-full gap-3">
          <div className="flex justify-center gap-2">
            <Button
              size="sm"
              onClick={() => void chooseFile()}
              disabled={isImporting}
            >
              <FileText className="size-4" /> 导入文件
            </Button>
            <Button
              size="sm"
              variant="outline"
              onClick={() => void chooseFolder()}
              disabled={isImporting}
            >
              <FolderOpen className="size-4" /> 导入文件夹
            </Button>
          </div>
          <div className="flex items-center gap-2 rounded-lg border border-border/70 bg-background px-2 py-2">
            <FilePlus2 className="ml-1 size-4 shrink-0 text-muted-foreground" />
            <input
              value={newFilename}
              onChange={(event) => setNewFilename(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  void createFile();
                }
              }}
              placeholder="新建文件名，例如 notes.txt"
              className="h-8 min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-muted-foreground"
            />
            <Button
              size="sm"
              variant="secondary"
              onClick={() => void createFile()}
              disabled={!canCreateFile}
            >
              新建文件
            </Button>
          </div>
        </div>
        {importError && (
          <p className="text-center text-xs text-destructive">{importError}</p>
        )}
      </div>
    </div>
  );
}
