import { createHashRouter } from "react-router-dom";
import { AppShell } from "./AppShell";
import { HomePage } from "../features/home/HomePage";
import { ImportPage } from "../features/import/ImportPage";
import { JobsPage } from "../features/jobs/JobsPage";
import { TranslationPreviewPage } from "../features/preview/TranslationPreviewPage";
import { SettingsPage } from "../features/settings/SettingsPage";

export const router = createHashRouter([
  {
    path: "/preview/:jobId/translations/:translationFileId",
    element: <TranslationPreviewPage />,
  },
  {
    path: "/",
    element: <AppShell />,
    children: [
      { index: true, element: <HomePage /> },
      { path: "new", element: <ImportPage /> },
      { path: "jobs", element: <JobsPage /> },
      { path: "jobs/:jobId/files/:fileId", element: <JobsPage /> },
      { path: "jobs/:jobId", element: <JobsPage /> },
      { path: "settings", element: <SettingsPage /> },
    ],
  },
]);
