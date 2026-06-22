import { createClient } from "npm:@supabase/supabase-js@2";

import { compareSemver, newestByVersion } from "../_shared/semver.ts";

type ReleaseRow = {
  version: string;
  storage_bucket: string;
  storage_path: string;
  signature: string;
  notes: string;
  pub_date: string;
};

const corsHeaders = {
  "access-control-allow-origin": "*",
  "access-control-allow-methods": "GET, OPTIONS",
  "access-control-allow-headers": "authorization, x-client-info, apikey, content-type",
};

function noContent(): Response {
  return new Response(null, {
    status: 204,
    headers: corsHeaders,
  });
}

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
    return new Response(null, {
      status: 204,
      headers: corsHeaders,
    });
  }

  if (request.method !== "GET") {
    return jsonResponse({ error: "Method not allowed" }, 405);
  }

  const url = new URL(request.url);
  const target = url.searchParams.get("target") ?? "";
  const arch = url.searchParams.get("arch") ?? "";
  const currentVersion = url.searchParams.get("current_version") ?? "0.0.0";

  if (!isSupportedPlatform(target, arch)) {
    return noContent();
  }

  const supabaseUrl = Deno.env.get("SUPABASE_URL");
  const serviceRoleKey = Deno.env.get("SUPABASE_SERVICE_ROLE_KEY");

  if (!supabaseUrl || !serviceRoleKey) {
    return jsonResponse({ error: "Updater service is not configured" }, 500);
  }

  const supabase = createClient(supabaseUrl, serviceRoleKey, {
    auth: {
      persistSession: false,
      autoRefreshToken: false,
    },
  });

  const { data, error } = await supabase
    .from("app_releases")
    .select("version, storage_bucket, storage_path, signature, notes, pub_date")
    .eq("app", "rosetta")
    .eq("target", target)
    .eq("arch", arch)
    .eq("is_published", true);

  if (error) {
    return jsonResponse({ error: "Could not load release metadata" }, 500);
  }

  const release = newestByVersion(
    ((data ?? []) as ReleaseRow[]).filter(
      (candidate) => compareSemver(candidate.version, currentVersion) > 0,
    ),
  );

  if (!release) {
    return noContent();
  }

  const { data: signedUrl, error: signedUrlError } = await supabase.storage
    .from(release.storage_bucket)
    .createSignedUrl(release.storage_path, 60 * 30);

  if (signedUrlError || !signedUrl?.signedUrl) {
    return jsonResponse({ error: "Could not create update download URL" }, 500);
  }

  return jsonResponse({
    version: release.version,
    pub_date: release.pub_date,
    url: signedUrl.signedUrl,
    signature: release.signature,
    notes: release.notes,
  });
});
