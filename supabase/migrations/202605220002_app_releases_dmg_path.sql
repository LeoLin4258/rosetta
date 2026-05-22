alter table public.app_releases
  add column if not exists dmg_storage_path text;
