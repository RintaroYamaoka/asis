-- Supabase が用意するもの(役・auth の関数と表)の最小の代わり。試験のときだけ使う。
create role anon nologin; create role authenticated nologin;
create schema auth;
create table auth.identities (user_id uuid, provider text, provider_id text);
create function auth.uid() returns uuid language sql stable as $$
  select nullif(current_setting('request.jwt.claim.sub', true), '')::uuid
$$;
grant usage on schema auth to authenticated, anon;
grant usage on schema public to authenticated, anon;
