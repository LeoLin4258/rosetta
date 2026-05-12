import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { FileText, FolderOpen } from "lucide-react";
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import {
  importRosettaDocumentFromPath,
  importRosettaProjectFromDirectory,
  pickRosettaImportDirectory,
  pickRosettaImportPath,
} from "../../lib/rosettaJobs";
import { rosettaJobDefaultPath } from "../../lib/rosettaRoutes";
import { useRosettaStore } from "../../store/useRosettaStore";

export function ImportPage() {
  const navigate = useNavigate();
  const setActiveBundle = useRosettaStore((state) => state.setActiveBundle);
  const [isImporting, setIsImporting] = useState(false);
  const [importError, setImportError] = useState<string | null>(null);

  async function chooseFile() {
    setIsImporting(true);
    setImportError(null);

    try {
      const selectedPath = await pickRosettaImportPath();

      if (!selectedPath) {
        return;
      }

      const bundle = await importRosettaDocumentFromPath(selectedPath);
      setActiveBundle(bundle);
      navigate(rosettaJobDefaultPath(bundle.job));
    } catch (error) {
      setImportError(
        error instanceof Error ? error.message : "无法导入这个文件。"
      );
    } finally {
      setIsImporting(false);
    }
  }

  async function chooseDirectory() {
    setIsImporting(true);
    setImportError(null);

    try {
      const selectedPath = await pickRosettaImportDirectory();

      if (!selectedPath) {
        return;
      }

      const bundle = await importRosettaProjectFromDirectory(selectedPath);
      setActiveBundle(bundle);
      navigate(rosettaJobDefaultPath(bundle.job));
    } catch (error) {
      setImportError(
        error instanceof Error ? error.message : "无法导入这个文件夹。"
      );
    } finally {
      setIsImporting(false);
    }
  }

  return (
    <section className="mx-auto flex max-w-3xl flex-col gap-6 px-6 py-6">
      <Card>
        <CardHeader>
          <CardTitle>新项目</CardTitle>
          <CardDescription>
            当前支持 TXT 和 Markdown；PDF 与 Word 支持规划中。
          </CardDescription>
          <CardAction>
            <FileText className="text-muted-foreground" />
          </CardAction>
        </CardHeader>

        <CardContent>
          <div className="flex flex-col items-center justify-center rounded-lg border border-dashed bg-background px-5 py-12 text-center">
            <FolderOpen className="text-muted-foreground" />
            <div className="mt-3 text-sm text-muted-foreground">
              选择一个文件夹，Rosetta 会把里面的 TXT 和 Markdown 作为同一个项目。
            </div>
            <div className="mt-5 flex flex-wrap justify-center gap-2">
              <Button
                disabled={isImporting}
                onClick={() => void chooseDirectory()}
                type="button"
              >
                <FolderOpen data-icon="inline-start" />
                {isImporting ? "导入中" : "选择文件夹"}
              </Button>
              <Button
                disabled={isImporting}
                onClick={() => void chooseFile()}
                type="button"
                variant="outline"
              >
                <FileText data-icon="inline-start" />
                选择单个文件
              </Button>
            </div>
            {importError ? (
              <p className="mt-4 text-sm text-destructive">{importError}</p>
            ) : null}
          </div>
        </CardContent>
      </Card>
    </section>
  );
}
