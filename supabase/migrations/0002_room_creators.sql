-- この DB で部屋を作ってよい人。載っていない人は、招かれた部屋に入れるだけ。
-- DB を動かしている人が、勝手に使われないための守り。表は API からは触れない。
create table public.room_creators (
  github_id  bigint primary key,
  created_at timestamptz not null default now()
);
alter table public.room_creators enable row level security;
revoke all on public.room_creators from public, anon, authenticated;

create or replace function public.create_room(room_name text, my_login text) returns uuid
language plpgsql security definer set search_path = '' as $$
declare
  me bigint := public.github_id();
  new_id uuid;
begin
  if me is null then
    raise exception 'GitHub でログインしていません' using errcode = '28000';
  end if;
  if not exists (select 1 from public.room_creators c where c.github_id = me) then
    raise exception 'この DB では、部屋を作る許可がありません' using errcode = '42501';
  end if;
  insert into public.rooms (name) values (room_name) returning id into new_id;
  insert into public.room_members (room_id, github_id, login, is_admin, review, merge, deploy)
  values (new_id, me, my_login, true, true, true, true);
  return new_id;
end
$$;
