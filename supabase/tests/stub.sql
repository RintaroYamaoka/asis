-- Supabase が用意するもの(役・auth の関数と表)の最小の代わり。試験のときだけ使う。
create role anon nologin; create role authenticated nologin;
create schema auth;
create table auth.identities (user_id uuid, provider text, provider_id text, identity_data jsonb);
create function auth.uid() returns uuid language sql stable as $$
  select nullif(current_setting('request.jwt.claim.sub', true), '')::uuid
$$;
grant usage on schema auth to authenticated, anon;
grant usage on schema public to authenticated, anon;
-- ファイルの置き場(storage)の最小の代わり
create schema storage;
create table storage.buckets (id text primary key, name text, public boolean);
create table storage.objects (bucket_id text, name text);
alter table storage.objects enable row level security;
grant usage on schema storage to authenticated, anon;
grant select, insert, update, delete on storage.objects to authenticated;
