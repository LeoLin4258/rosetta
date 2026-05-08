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
import { Badge } from "@/components/ui/badge";
import { Separator } from "@/components/ui/separator";
import {
  importRosettaDocumentFromPath,
  pickRosettaImportPath,
} from "../../lib/rosettaJobs";
import { useRosettaStore } from "../../store/useRosettaStore";

export function ImportPage() {
  const navigate = useNavigate();
  const rwkv = useRosettaStore((state) => state.rwkv);
  const setActiveBundle = useRosettaStore((state) => state.setActiveBundle);
  const [isImporting, setIsImporting] = useState(false);
  const [importError, setImportError] = useState<string | null>(null);
  const rwkvReady =
    rwkv.baseUrl.trim().length > 0 &&
    rwkv.endpoint.trim().length > 0 &&
    rwkv.internalToken.trim().length > 0 &&
    rwkv.bodyPassword.trim().length > 0;

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
      navigate(`/jobs/${bundle.job.id}`);
    } catch (error) {
      setImportError(
        error instanceof Error ? error.message : "无法导入这个文件。"
      );
    } finally {
      setIsImporting(false);
    }
  }

  return (
    <section className="mx-auto flex max-w-5xl flex-col gap-6 px-6 py-6">
      <div className="grid gap-4 md:grid-cols-[1.4fr_1fr]">
        <Card>
          <CardHeader>
            <CardTitle>导入文档</CardTitle>
            <CardDescription>当前支持 TXT、Markdown</CardDescription>
            <CardAction>
              <FileText className="size-5 text-muted-foreground" />
            </CardAction>
          </CardHeader>

          <CardContent>
            <div className="rounded-lg border border-dashed bg-background px-5 py-10 text-center">
              <FolderOpen className="mx-auto size-8 text-muted-foreground" />
              <div className="mt-3 text-sm text-muted-foreground">
                选择本地 TXT 或 Markdown 文件创建项目
              </div>
              <Button
                className="mt-4"
                disabled={isImporting}
                onClick={() => void chooseFile()}
                type="button"
              >
                {isImporting ? "导入中" : "选择文件"}
              </Button>
              {importError ? (
                <p className="mt-4 text-sm text-destructive">{importError}</p>
              ) : null}
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>当前流程</CardTitle>
            <CardDescription>导入后会生成本机 JSON 项目缓存</CardDescription>
          </CardHeader>
          <CardContent>
            <div className="flex flex-col gap-3 text-sm text-muted-foreground">
              <div className="flex items-center justify-between">
                <span>RWKV API</span>
                <Badge variant={rwkvReady ? "secondary" : "outline"}>
                  {rwkvReady ? "已配置" : "待配置"}
                </Badge>
              </div>
              <Separator />
              <div className="flex items-center justify-between">
                <span>导入格式</span>
                <span className="text-foreground">TXT / Markdown</span>
              </div>
              <Separator />
              <div className="flex items-center justify-between">
                <span>任务缓存</span>
                <span className="text-foreground">本机 JSON</span>
              </div>
              <Separator />
              <div className="flex items-center justify-between">
                <span>导出</span>
                <span className="text-foreground">译文 / 双语对照</span>
              </div>
            </div>
          </CardContent>
        </Card>
      </div>
    </section>
  );
}
