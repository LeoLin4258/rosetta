import { createClient } from "npm:@supabase/supabase-js@2";

const corsHeaders = {
  "access-control-allow-origin": "*",
  "access-control-allow-methods": "GET, OPTIONS",
  "access-control-allow-headers": "authorization, x-client-info, apikey, content-type",
};

Deno.serve(async (request) => {
  if (request.method === "OPTIONS") {
    return new Response(null, { status: 204, headers: corsHeaders });
  }

  if (request.method !== "GET") {
    return new Response(JSON.stringify({ error: "Method not allowed" }), {
      status: 405,
      headers: { ...corsHeaders, "content-type": "application/json; charset=utf-8" },
    });
  }

  const supabaseUrl = Deno.env.get("SUPABASE_URL");
  const serviceRoleKey = Deno.env.get("SUPABASE_SERVICE_ROLE_KEY");

  if (!supabaseUrl || !serviceRoleKey) {
    return new Response(JSON.stringify({ error: "Service not configured" }), {
      status: 500,
      headers: { ...corsHeaders, "content-type": "application/json; charset=utf-8" },
    });
  }

  const supabase = createClient(supabaseUrl, serviceRoleKey, {
    auth: { persistSession: false, autoRefreshToken: false },
  });

  const { data, error } = await supabase
    .from("app_releases")
    .select("version, dmg_storage_path, storage_bucket, pub_date")
    .eq("app", "rosetta")
    .eq("target", "darwin")
    .eq("arch", "aarch64")
    .eq("is_published", true)
    .not("dmg_storage_path", "is", null)
    .order("pub_date", { ascending: false })
    .limit(1)
    .single();

  if (error || !data) {
    return new Response(JSON.stringify({ error: "No release available" }), {
      status: 404,
      headers: { ...corsHeaders, "content-type": "application/json; charset=utf-8", "cache-control": "no-store" },
    });
  }

  const { data: signedUrl, error: signedUrlError } = await supabase.storage
    .from(data.storage_bucket)
    .createSignedUrl(data.dmg_storage_path, 60 * 60);

  if (signedUrlError || !signedUrl?.signedUrl) {
    return new Response(JSON.stringify({ error: "Could not create download URL" }), {
      status: 500,
      headers: { ...corsHeaders, "content-type": "application/json; charset=utf-8" },
    });
  }

  return new Response(
    JSON.stringify({
      version: data.version,
      pub_date: data.pub_date,
      url: signedUrl.signedUrl,
    }),
    {
      status: 200,
      headers: {
        ...corsHeaders,
        "content-type": "application/json; charset=utf-8",
        "cache-control": "no-store",
      },
    },
  );
});
