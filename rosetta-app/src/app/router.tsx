import { createHashRouter } from "react-router-dom";
import { AppShell } from "./AppShell";
import { SourcePreviewPage } from "../features/preview/SourcePreviewPage";
import { TranslationPreviewPage } from "../features/preview/TranslationPreviewPage";
import { SettingsPage } from "../features/settings/SettingsPage";
import { WorkspacePage } from "../features/workspace/WorkspacePage";

export const router = createHashRouter([
  {
    path: "/preview/:jobId/translations/:translationFileId",
    element: <TranslationPreviewPage />,
  },
  {
    path: "/preview/:jobId/sources/:sourceFileId",
    element: <SourcePreviewPage />,
  },
  {
    path: "/",
    element: <AppShell />,
    children: [
      { index: true, element: <WorkspacePage /> },
      { path: "settings", element: <SettingsPage /> },
    ],
  },
]);
