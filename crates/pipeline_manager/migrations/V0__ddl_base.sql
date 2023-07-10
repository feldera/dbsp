CREATE TABLE IF NOT EXISTS tenant (
    id uuid PRIMARY KEY,
    tenant varchar NOT NULL,
    provider varchar NOT NULL,
    UNIQUE (tenant, provider)
);

CREATE TABLE IF NOT EXISTS program (
    id uuid PRIMARY KEY,
    version bigint NOT NULL,
    tenant_id uuid NOT NULL,
    name varchar NOT NULL,
    description varchar NOT NULL,
    code varchar NOT NULL,
    schema varchar,
    status varchar,
    error varchar,
    status_since timestamp NOT NULL,
    FOREIGN KEY (tenant_id) REFERENCES tenant(id) ON DELETE CASCADE,
    CONSTRAINT unique_program_id UNIQUE (id, tenant_id),
    CONSTRAINT unique_program_name UNIQUE (tenant_id, name)
);
CREATE TABLE IF NOT EXISTS program_history (
    revision uuid,
    LIKE program,
    PRIMARY KEY (id, revision),
    FOREIGN KEY (tenant_id) REFERENCES tenant(id) ON DELETE CASCADE,
    CONSTRAINT unique_program_history_id UNIQUE(id, revision, tenant_id),
    CONSTRAINT unique_program_history_name UNIQUE(tenant_id, revision, name)
);

CREATE TABLE IF NOT EXISTS pipeline (
    id uuid PRIMARY KEY,
    program_id uuid,
    version bigint NOT NULL,
    tenant_id uuid NOT NULL,
    name varchar NOT NULL,
    description varchar NOT NULL,
    config varchar NOT NULL,
    last_revision uuid,
    FOREIGN KEY (program_id, tenant_id) REFERENCES program(id, tenant_id) ON DELETE CASCADE,
    CONSTRAINT unique_pipeline_name UNIQUE(tenant_id, name)
);

CREATE TABLE IF NOT EXISTS pipeline_runtime_state (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    -- TODO: add 'host' field when we support remote pipelines.
    port smallint,
    desired_status varchar NOT NULL,
    current_status varchar NOT NULL,
    status_since bigint NOT NULL,
    error varchar,
    created,
    FOREIGN KEY id REFERENCES pipeline(id) ON DELETE CASCADE,
);

CREATE TABLE IF NOT EXISTS pipeline_history (
    revision uuid,
    LIKE pipeline,
    PRIMARY KEY (id, revision),
    CONSTRAINT unique_pipeline_history_name UNIQUE(tenant_id, revision, name)
);

CREATE TABLE IF NOT EXISTS connector (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    name varchar NOT NULL,
    description varchar NOT NULL,
    config varchar NOT NULL,
    FOREIGN KEY (tenant_id) REFERENCES tenant(id) ON DELETE CASCADE,
    CONSTRAINT unique_connector_name UNIQUE(tenant_id, name)
);
CREATE TABLE IF NOT EXISTS connector_history (
    revision uuid,
    LIKE connector,
    PRIMARY KEY (id, revision),
    FOREIGN KEY (tenant_id) REFERENCES tenant(id) ON DELETE CASCADE,
    CONSTRAINT unique_connector_history_name UNIQUE(tenant_id, revision, name)
);

CREATE TABLE IF NOT EXISTS attached_connector (
    pipeline_id uuid NOT NULL,
    connector_id uuid NOT NULL,
    tenant_id uuid NOT NULL,
    name varchar,
    config varchar,
    is_input bool NOT NULL,
    PRIMARY KEY (pipeline_id, name),
    FOREIGN KEY (pipeline_id) REFERENCES pipeline(id) ON DELETE CASCADE,
    FOREIGN KEY (connector_id) REFERENCES connector(id) ON DELETE CASCADE,
    FOREIGN KEY (tenant_id) REFERENCES tenant(id) ON DELETE CASCADE
);
CREATE TABLE IF NOT EXISTS attached_connector_history (
    revision uuid,
    LIKE attached_connector,
    PRIMARY KEY (pipeline_id, name, revision),
    FOREIGN KEY (pipeline_id, revision) REFERENCES pipeline_history(id, revision) ON DELETE CASCADE,
    FOREIGN KEY (connector_id, revision) REFERENCES connector_history(id, revision) ON DELETE CASCADE,
    FOREIGN KEY (tenant_id) REFERENCES tenant(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS api_key (
    hash varchar PRIMARY KEY,
    tenant_id uuid NOT NULL,
    scopes text[] NOT NULL,
    FOREIGN KEY (tenant_id) REFERENCES tenant(id) ON DELETE CASCADE
);
