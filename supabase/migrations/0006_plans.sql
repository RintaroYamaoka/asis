-- 外から送られてくる計画。計画の正本が GitHub に無い人が、その人の道具から部屋に送る(crates/asis/src/plans.rs)。
-- 入るのは、送った人が書いた計画の 1 件(番号と、送られてきたままの中身)。issue・PR・コードは、今までどおり入れない。
create table public.plans (
  room_id    uuid not null references public.rooms(id) on delete cascade,
  key        text not null check (char_length(key) between 1 and 80),  -- 番号。部屋の中で 1 つ
  data       jsonb not null check (jsonb_typeof(data) = 'object' and pg_column_size(data) < 20000),
  sent_by    bigint,                                                  -- 最後に送った人(GitHub の番号)
  updated_at timestamptz not null default now(),
  primary key (room_id, key)
);

-- 誰がいつ送ったかは、送る側に書かせない
create function public.stamp_plan() returns trigger
language plpgsql security definer set search_path = '' as $$
begin
  new.sent_by := public.github_id();
  new.updated_at := now();
  return new;
end
$$;
revoke execute on function public.stamp_plan() from public, anon, authenticated;
create trigger plans_stamp before insert or update on public.plans
  for each row execute function public.stamp_plan();

alter table public.plans enable row level security;
revoke all on public.plans from public, anon, authenticated;
grant select, insert, update, delete on public.plans to authenticated;

-- 読むのも送るのも、その部屋のメンバーだけ。役は問わない(計画を書くのは、役を持たない人のこともある)
create policy plans_read   on public.plans for select to authenticated using (public.is_member(room_id));
create policy plans_send   on public.plans for insert to authenticated with check (public.is_member(room_id));
create policy plans_change on public.plans for update to authenticated
  using (public.is_member(room_id)) with check (public.is_member(room_id));
create policy plans_remove on public.plans for delete to authenticated using (public.is_member(room_id));

insert into public.schema_migrations (version) values ('0006');
