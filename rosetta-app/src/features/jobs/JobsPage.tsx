import { SegmentPreviewList } from "../preview/SegmentPreviewList";
import { useRosettaStore } from "../../store/useRosettaStore";

export function JobsPage() {
  const jobs = useRosettaStore((state) => state.jobs);

  return (
    <section className="grid min-h-full grid-rows-[auto_1fr] gap-6 px-6 py-6">
      <div className="overflow-hidden rounded-lg border border-zinc-800">
        <table className="w-full border-collapse text-left text-sm">
          <thead className="bg-zinc-900 text-zinc-400">
            <tr>
              <th className="px-4 py-3 font-medium">文件</th>
              <th className="px-4 py-3 font-medium">状态</th>
              <th className="px-4 py-3 font-medium">进度</th>
              <th className="px-4 py-3 font-medium">失败</th>
              <th className="px-4 py-3 font-medium">更新时间</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-zinc-800 bg-zinc-950">
            {jobs.map((job) => (
              <tr className="text-zinc-300" key={job.id}>
                <td className="px-4 py-3 text-zinc-100">{job.filename}</td>
                <td className="px-4 py-3">{job.status}</td>
                <td className="px-4 py-3">
                  {job.completedSegments} / {job.segmentCount}
                </td>
                <td className="px-4 py-3">{job.failedSegments}</td>
                <td className="px-4 py-3 text-zinc-500">
                  {new Date(job.updatedAt).toLocaleString()}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <SegmentPreviewList />
    </section>
  );
}
