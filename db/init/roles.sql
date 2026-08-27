-- Runs once when the local Postgres container initialises (docker-entrypoint-initdb.d).
-- Creates the three least-privilege roles the services connect with. The
-- application role owns the schema; migrations grant the others what they need.
create role bowline_app    login createdb password 'bowline_app_dev';
create role bowline_ro     login password 'bowline_ro_dev';
create role bowline_notify login password 'bowline_notify_dev';
create database bowline owner bowline_app;
\connect bowline
grant all on schema public to bowline_app;
alter default privileges for role bowline_app in schema public grant select on tables to bowline_ro;
alter default privileges for role bowline_app in schema public grant select on sequences to bowline_ro;
