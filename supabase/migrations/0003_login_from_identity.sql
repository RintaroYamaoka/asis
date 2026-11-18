-- 部屋を作る人の名前も、本人が渡す値ではなく、認証の記録から読む。
-- 役割は名前で PR の事実と照らすので、名前を偽れると役割を偽れる。
create function public.github_login() returns text
language sql stable security definer set search_path = '' as $$
  select i.identity_data->>'user_name' from auth.identities i
  where i.user_id = auth.uid() and i.provider = 'github'
  limit 1
$$;
revoke execute on function public.github_login() from public, anon;
grant execute on function public.github_login() to authenticated;

drop function public.create_room(text, text);

create function public.create_room(room_name text) returns uuid
language plpgsql security definer set search_path = '' as $$
declare
  me bigint := public.github_id();
  my_login text := public.github_login();
  new_id uuid;
begin
  if me is null or my_login is null then
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
revoke execute on function public.create_room(text) from public, anon;
grant execute on function public.create_room(text) to authenticated;
