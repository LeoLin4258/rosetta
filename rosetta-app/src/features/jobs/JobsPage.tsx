import { SegmentPreviewList } from "../preview/SegmentPreviewList";
import { useRosettaStore } from "../../store/useRosettaStore";
import { Badge } from "@/components/ui/badge";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";

export function JobsPage() {
  const jobs = useRosettaStore((state) => state.jobs);

  return (
    <section className="grid min-h-full grid-rows-[auto_1fr] gap-6 px-6 py-6">
      <div className="overflow-hidden rounded-lg border bg-card">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead className="px-4">文件</TableHead>
              <TableHead>状态</TableHead>
              <TableHead>进度</TableHead>
              <TableHead>失败</TableHead>
              <TableHead>更新时间</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {jobs.map((job) => (
              <TableRow key={job.id}>
                <TableCell className="px-4 font-medium">{job.filename}</TableCell>
                <TableCell>
                  <Badge variant="secondary">{job.status}</Badge>
                </TableCell>
                <TableCell>
                  {job.completedSegments} / {job.segmentCount}
                </TableCell>
                <TableCell>{job.failedSegments}</TableCell>
                <TableCell className="text-muted-foreground">
                  {new Date(job.updatedAt).toLocaleString()}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>

      <SegmentPreviewList />
    </section>
  );
}
