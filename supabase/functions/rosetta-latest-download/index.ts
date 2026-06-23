import { createClient } from "npm:@supabase/supabase-js@2";

import { newestByVersion } from "../_shared/semver.ts";

type ReleaseRow = {
  version: string;
  target: string;
  arch: string;
  storage_bucket: string;
  installer_storage_path: string;
  installer_sha256: string | null;
  installer_size_bytes: number | null;
  pub_date: string;
};

const corsHeaders = {
  "access-control-allow-origin": "*",
  "access-control-allow-methods": "GET, OPTIONS",
  "access-control-allow-headers":
    "authorization, x-client-info, apikey, content-type",
};

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: {
      ...corsHeaders,
      "content-type": "application/json; charset=utf-8",
      "cache-control": "no-store",
    },
  });
}

function isSupportedPlatform(target: string, arch: string): boolean {
  return (
    (target === "darwin" && arch === "aarch64") ||
    (target === "windows" && arch === "x86_64")
  );
}

Deno.serve(async (request) => {
  if (request.method === "OPTIONS") {
    return new Response(null, { status: 204, headers: corsHeaders });
  }
  if (request.method !== "GET") {
    return jsonResponse({ error: "Method not allowed" }, 405);
  }

  const url = new URL(request.url);
  const target = url.searchParams.get("target") ?? "";
  const arch = url.searchParams.get("arch") ?? "";

  if (!isSupportedPlatform(target, arch)) {
    return jsonResponse({ error: "Unsupported platform" }, 404);
  }

  const supabaseUrl = Deno.env.get("SUPABASE_URL");
  const serviceRoleKey = Deno.env.get("SUPABASE_SERVICE_ROLE_KEY");
  if (!supabaseUrl || !serviceRoleKey) {
    return jsonResponse({ error: "Download service is not configured" }, 500);
  }

  const supabase = createClient(supabaseUrl, serviceRoleKey, {
    auth: { persistSession: false, autoRefreshToken: false },
  });
  const { data, error } = await supabase
    .from("app_releases")
    .select(
      "version, target, arch, storage_bucket, installer_storage_path, installer_sha256, installer_size_bytes, pub_date",
    )
    .eq("app", "rosetta")
    .eq("target", target)
    .eq("arch", arch)
    .eq("is_published", true)
    .not("installer_storage_path", "is", null);

  if (error) {
    return jsonResponse({ error: "Could not load release metadata" }, 500);
  }

  const release = newestByVersion((data ?? []) as ReleaseRow[]);
  if (!release) {
    return jsonResponse({ error: "No release available" }, 404);
  }

  if (url.searchParams.get("download") === "1") {
    const { data: signedUrl, error: signedUrlError } = await supabase.storage
      .from(release.storage_bucket)
      .createSignedUrl(release.installer_storage_path, 60 * 10);

    if (signedUrlError || !signedUrl?.signedUrl) {
      return jsonResponse({ error: "Could not create download URL" }, 500);
    }

    return Response.redirect(signedUrl.signedUrl, 302);
  }

  const filename =
    release.installer_storage_path.split("/").at(-1) ?? "Rosetta-installer";
  const downloadUrl = new URL(
    "/functions/v1/rosetta-latest-download",
    supabaseUrl,
  );
  downloadUrl.searchParams.set("target", target);
  downloadUrl.searchParams.set("arch", arch);
  downloadUrl.searchParams.set("download", "1");

  return jsonResponse({
    version: release.version,
    pub_date: release.pub_date,
    target: release.target,
    arch: release.arch,
    filename,
    size_bytes: release.installer_size_bytes,
    sha256: release.installer_sha256,
    download_url: downloadUrl.toString(),
  });
});
