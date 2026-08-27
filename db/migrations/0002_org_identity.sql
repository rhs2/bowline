-- ============================================================================
-- Organisation: departments, positions, employees (with the ltree chain of command)
-- Identity: users, roles, permissions, refresh tokens
-- ============================================================================

create table departments (
  id          uuid primary key default gen_random_uuid(),
  code        citext not null unique,
  name        text   not null,
  parent_id   uuid references departments(id) on delete restrict,
  cost_center text,
  created_at  timestamptz not null default now(),
  updated_at  timestamptz not null default now()
);
create trigger departments_updated before update on departments for each row execute function set_updated_at();

create table positions (
  id                uuid primary key default gen_random_uuid(),
  code              citext not null unique,
  title             text   not null,
  level             smallint not null check (level between 1 and 7),
  department_id     uuid references departments(id) on delete restrict,
  is_people_manager boolean not null default false,
  created_at        timestamptz not null default now()
);

create table employees (
  id               uuid primary key default gen_random_uuid(),
  employee_no      text   not null unique,
  first_name       text   not null,
  last_name        text   not null,
  email            citext not null unique,
  phone            text,
  position_id      uuid   not null references positions(id) on delete restrict,
  department_id    uuid   not null references departments(id) on delete restrict,
  manager_id       uuid   references employees(id) on delete restrict,
  path             ltree  not null,
  status           text   not null default 'active'
                   check (status in ('active','on_leave','suspended','terminated')),
  employment_type  text   not null default 'full_time'
                   check (employment_type in ('full_time','part_time','contract')),
  hire_date        date   not null,
  termination_date date,
  site             text,
  pay_grade        text,
  base_salary      numeric(12,2) not null default 0 check (base_salary >= 0),
  currency         char(3) not null default 'USD',
  created_at       timestamptz not null default now(),
  updated_at       timestamptz not null default now(),
  check (manager_id is distinct from id),
  check ((status = 'terminated') = (termination_date is not null))
);
create index employees_path_gist on employees using gist (path);
create index employees_manager_idx on employees (manager_id);
create index employees_department_idx on employees (department_id);
create index employees_status_idx on employees (status);
create trigger employees_updated before update on employees for each row execute function set_updated_at();

-- ltree labels may only contain [A-Za-z0-9_-]; uuids are safe once hyphens become underscores.
create or replace function employee_label(p_id uuid) returns ltree
language sql immutable as $$ select text2ltree(replace(p_id::text, '-', '_')) $$;

-- BEFORE trigger: compute this employee's own path from the manager's path.
create or replace function employees_path_before() returns trigger
language plpgsql as $$
declare
  parent_path ltree;
begin
  if tg_op = 'UPDATE' and new.manager_id is not distinct from old.manager_id and new.id = old.id then
    -- manager unchanged: path is only rewritten by the cascade below, never by hand
    if new.path <> old.path and pg_trigger_depth() <= 1 then
      raise exception 'employees.path is derived from manager_id and cannot be set directly';
    end if;
    return new;
  end if;
  if new.manager_id is null then
    if exists (select 1 from employees where manager_id is null and id <> new.id and status <> 'terminated') then
      raise exception 'only one employee (the CEO) may have no manager';
    end if;
    new.path := employee_label(new.id);
    return new;
  end if;
  select path into parent_path from employees where id = new.manager_id;
  if parent_path is null then
    raise exception 'manager % does not exist', new.manager_id;
  end if;
  if tg_op = 'UPDATE' and parent_path <@ old.path then
    raise exception 'reporting cycle: % cannot report to someone in their own subtree', new.id;
  end if;
  new.path := parent_path || employee_label(new.id);
  return new;
end $$;
create trigger employees_path_before before insert or update of manager_id, path on employees
  for each row execute function employees_path_before();

-- AFTER trigger: when a path changes, rewrite every descendant in one statement.
create or replace function employees_path_cascade() returns trigger
language plpgsql as $$
begin
  if new.path <> old.path and pg_trigger_depth() <= 1 then
    update employees
       set path = new.path || subpath(path, nlevel(old.path))
     where path <@ old.path and id <> new.id;
  end if;
  return null;
end $$;
create trigger employees_path_cascade after update of manager_id, path on employees
  for each row execute function employees_path_cascade();

-- ---------------------------------------------------------------------------
-- Identity
-- ---------------------------------------------------------------------------
create table users (
  id                   uuid primary key default gen_random_uuid(),
  employee_id          uuid not null unique references employees(id) on delete restrict,
  email                citext not null unique,
  password_hash        text not null,
  status               text not null default 'active' check (status in ('active','locked','disabled')),
  failed_logins        integer not null default 0,
  locked_until         timestamptz,
  must_change_password boolean not null default true,
  token_version        integer not null default 1,
  last_login_at        timestamptz,
  created_at           timestamptz not null default now(),
  updated_at           timestamptz not null default now()
);
create trigger users_updated before update on users for each row execute function set_updated_at();

create table roles (
  id          smallserial primary key,
  key         citext not null unique,
  name        text not null,
  description text not null default ''
);

create table permissions (
  key         citext primary key,
  description text not null default ''
);

create table role_permissions (
  role_id        smallint not null references roles(id) on delete cascade,
  permission_key citext   not null references permissions(key) on delete cascade,
  primary key (role_id, permission_key)
);

create table user_roles (
  user_id    uuid     not null references users(id) on delete cascade,
  role_id    smallint not null references roles(id) on delete cascade,
  granted_by uuid     references users(id) on delete set null,
  granted_at timestamptz not null default now(),
  primary key (user_id, role_id)
);

create table refresh_tokens (
  id          uuid primary key default gen_random_uuid(),
  user_id     uuid not null references users(id) on delete cascade,
  family_id   uuid not null,                    -- one family per login; reuse revokes the family
  token_hash  text not null unique,             -- sha256 of the opaque token
  expires_at  timestamptz not null,
  revoked_at  timestamptz,
  replaced_by uuid references refresh_tokens(id) on delete set null,
  user_agent  text,
  ip          inet,
  created_at  timestamptz not null default now()
);
create index refresh_tokens_user_idx on refresh_tokens (user_id);
create index refresh_tokens_family_idx on refresh_tokens (family_id);

-- Effective permissions for a user, one row per key.
create view user_permissions as
  select ur.user_id, rp.permission_key
    from user_roles ur
    join role_permissions rp on rp.role_id = ur.role_id
   group by ur.user_id, rp.permission_key;
