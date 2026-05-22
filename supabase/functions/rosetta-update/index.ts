import { createClient } from "npm:@supabase/supabase-js@2";

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

function parseSemver(input: string): [number, number, number, string] | null {
  const match = input.match(/^v?(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?$/);
  if (!match) {
    return null;
  }

  return [
    Number.parseInt(match[1], 10),
    Number.parseInt(match[2], 10),
    Number.parseInt(match[3], 10),
    match[4] ?? "",
  ];
}

function comparePrerelease(left: string, right: string): number {
  if (left === right) {
    return 0;
  }

  if (left === "") {
    return 1;
  }

  if (right === "") {
    return -1;
  }

  const leftParts = left.split(".");
  const rightParts = right.split(".");
  const max = Math.max(leftParts.length, rightParts.length);

  for (let index = 0; index < max; index += 1) {
    const leftPart = leftParts[index];
    const rightPart = rightParts[index];

    if (leftPart === undefined) {
      return -1;
    }

    if (rightPart === undefined) {
      return 1;
    }

    const leftNumber = /^\d+$/.test(leftPart) ? Number.parseInt(leftPart, 10) : null;
    const rightNumber = /^\d+$/.test(rightPart) ? Number.parseInt(rightPart, 10) : null;

    if (leftNumber !== null && rightNumber !== null) {
      if (leftNumber !== rightNumber) {
        return leftNumber > rightNumber ? 1 : -1;
      }
      continue;
    }

    if (leftNumber !== null) {
      return -1;
    }

    if (rightNumber !== null) {
      return 1;
    }

    if (leftPart !== rightPart) {
      return leftPart > rightPart ? 1 : -1;
    }
  }

  return 0;
}

function compareSemver(left: string, right: string): number {
  const leftParsed = parseSemver(left);
  const rightParsed = parseSemver(right);

  if (!leftParsed || !rightParsed) {
    return left.localeCompare(right);
  }

  for (let index = 0; index < 3; index += 1) {
    if (leftParsed[index] !== rightParsed[index]) {
      return leftParsed[index] > rightParsed[index] ? 1 : -1;
    }
  }

  return comparePrerelease(leftParsed[3], rightParsed[3]);
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

  if (target !== "darwin" || arch !== "aarch64") {
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
    .eq("target", "darwin")
    .eq("arch", "aarch64")
    .eq("is_published", true);

  if (error) {
    return jsonResponse({ error: "Could not load release metadata" }, 500);
  }

  const candidates = ((data ?? []) as ReleaseRow[])
    .filter((release) => compareSemver(release.version, currentVersion) > 0)
    .sort((left, right) => compareSemver(right.version, left.version));

  const release = candidates[0];

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
