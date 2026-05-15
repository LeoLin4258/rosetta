import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { useNavigate } from "react-router-dom";
import { importRosettaDocumentFromPath, pickRosettaImportPath } from "./rosettaJobs";
import { useRosettaStore } from "@/store/useRosettaStore";

export function useMenuEvents(toggleSidebar: () => void) {
  const navigate = useNavigate();
  const setActiveBundle = useRosettaStore((s) => s.setActiveBundle);

  // Keep a ref so the listener closure always calls the latest toggleSidebar
  // without needing it in the effect's dependency array. toggleSidebar from
  // shadcn changes identity on every sidebar state change (its useCallback
  // deps include `open`), so including it in deps would re-register the
  // Tauri listener on every toggle.
  const toggleSidebarRef = useRef(toggleSidebar);
  useEffect(() => { toggleSidebarRef.current = toggleSidebar; }, [toggleSidebar]);

  useEffect(() => {
    let unmounted = false;
    let unlisten: (() => void) | null = null;

    listen<string>("rosetta-menu-event", async (event) => {
      switch (event.payload) {
        case "open-file": {
          try {
            const path = await pickRosettaImportPath();
            if (!path) break;
            const bundle = await importRosettaDocumentFromPath(path);
            setActiveBundle(bundle);
            navigate("/");
          } catch {
            // user cancelled or import failed — no-op
          }
          break;
        }
        case "preferences":
          navigate("/settings");
          break;
        case "toggle-sidebar":
          toggleSidebarRef.current();
          break;
      }
    }).then((fn) => {
      if (unmounted) {
        fn();
      } else {
        unlisten = fn;
      }
    }).catch(console.error);

    return () => {
      unmounted = true;
      unlisten?.();
    };
  }, [navigate, setActiveBundle]);
}
