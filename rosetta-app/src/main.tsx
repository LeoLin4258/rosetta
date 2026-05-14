import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import App from "./App";
import { OnboardingApp } from "./features/onboarding/OnboardingApp";
import "./styles/index.css";

// Rosetta runs in one of two Tauri windows — `main` (Workspace) or
// `onboarding` (first-launch wizard). Both load the same frontend bundle,
// so we branch on the window label here at the React root. The Rust setup
// hook decides which window to show; we just render the matching React tree.
const label = getCurrentWebviewWindow().label;

const RootComponent = label === "onboarding" ? OnboardingApp : App;

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <RootComponent />
  </React.StrictMode>,
);
