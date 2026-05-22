create extension if not exists pgcrypto;

insert into storage.buckets (id, name, public)
values ('rosetta-releases', 'rosetta-releases', false)
on conflict (id) do update set public = excluded.public;

create table if not exists public.app_releases (
  id uuid primary key default gen_random_uuid(),
  app text not null default 'rosetta',
  version text not null,
  target text not null,
  arch text not null,
  platform text generated always as (target || '-' || arch) stored,
  storage_bucket text not null default 'rosetta-releases',
  storage_path text not null,
  signature text not null,
  notes text not null default '',
  pub_date timestamptz not null default now(),
  is_published boolean not null default false,
  minimum_version text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  constraint app_releases_unique_version unique (app, version, target, arch),
  constraint app_releases_rosetta_macos_arm64_only check (
    app = 'rosetta'
    and target = 'darwin'
    and arch = 'aarch64'
    and storage_bucket = 'rosetta-releases'
  )
);

create index if not exists app_releases_lookup_idx
  on public.app_releases (app, target, arch, is_published, pub_date desc);

create or replace function public.set_app_releases_updated_at()
returns trigger
language plpgsql
as $$
begin
  new.updated_at = now();
  return new;
end;
$$;

drop trigger if exists app_releases_set_updated_at on public.app_releases;

create trigger app_releases_set_updated_at
before update on public.app_releases
for each row
execute function public.set_app_releases_updated_at();

alter table public.app_releases enable row level security;

drop policy if exists "No public app release reads" on public.app_releases;
create policy "No public app release reads"
on public.app_releases
for select
using (false);

drop policy if exists "No public app release writes" on public.app_releases;
create policy "No public app release writes"
on public.app_releases
for all
using (false)
with check (false);
