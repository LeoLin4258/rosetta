import { FileText, FolderOpen } from "lucide-react";
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import { useRosettaStore } from "../../store/useRosettaStore";

export function ImportPage() {
  const createDemoJob = useRosettaStore((state) => state.createDemoJob);

  return (
    <section className="mx-auto flex max-w-5xl flex-col gap-6 px-6 py-6">
      <div className="grid gap-4 md:grid-cols-[1.4fr_1fr]">
        <Card>
          <CardHeader>
            <CardTitle>导入文档</CardTitle>
            <CardDescription>TXT、Markdown、基础 DOCX</CardDescription>
            <CardAction>
              <FileText className="size-5 text-muted-foreground" />
            </CardAction>
          </CardHeader>

          <CardContent>
            <div className="rounded-lg border border-dashed bg-background px-5 py-10 text-center">
              <FolderOpen className="mx-auto size-8 text-muted-foreground" />
              <div className="mt-3 text-sm text-muted-foreground">
                拖入文件或选择本地文档
              </div>
              <Button className="mt-4" type="button">
                选择文件
              </Button>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>验证入口</CardTitle>
            <CardDescription>Stage 0 和 Stage 1 的连接状态</CardDescription>
          </CardHeader>
          <CardContent>
            <div className="flex flex-col gap-3 text-sm text-muted-foreground">
            <div className="flex items-center justify-between">
              <span>RWKV API</span>
              <span className="text-foreground">待连接</span>
            </div>
            <Separator />
            <div className="flex items-center justify-between">
              <span>翻译模式</span>
              <span className="text-foreground">平衡</span>
            </div>
            <Separator />
            <div className="flex items-center justify-between">
              <span>任务缓存</span>
              <span className="text-foreground">JSON</span>
            </div>
          </div>
          </CardContent>
          <CardFooter>
            <Button
            onClick={createDemoJob}
              variant="outline"
            type="button"
          >
            新建演示任务
            </Button>
          </CardFooter>
        </Card>
      </div>
    </section>
  );
}
