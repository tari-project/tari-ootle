create table watched_substates
(
    id                integer   not null primary key autoincrement,
    component_address text      not null unique,
    template_address  text      not null,
    created_at        timestamp not null default current_timestamp
);

create index idx_watched_substates_template on watched_substates (template_address);
create index idx_watched_substates_created_at on watched_substates (created_at);
