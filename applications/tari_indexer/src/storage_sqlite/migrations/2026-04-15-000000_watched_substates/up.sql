create table watched_substates
(
    component_address text not null,
    template_address  text not null,
    created_at        timestamp not null default current_timestamp,
    primary key (component_address)
);

create index idx_watched_substates_template on watched_substates (template_address);
