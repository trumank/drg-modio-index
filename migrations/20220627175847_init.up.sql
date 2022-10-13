CREATE TABLE IF NOT EXISTS mod (
    id_mod               INTEGER NOT NULL,
    id_modfile           INTEGER,
    name                 TEXT NOT NULL,
    name_id              TEXT NOT NULL,
    summary              TEXT NOT NULL,
    description          TEXT,
    PRIMARY KEY (id_mod),
    FOREIGN KEY (id_modfile) REFERENCES modfile (id_modfile) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE IF NOT EXISTS modfile (
    id_modfile           INTEGER NOT NULL,
    id_mod               INTEGER NOT NULL,
    date_added           TEXT NOT NULL,
    hash_md5             TEXT NOT NULL,
    filename             TEXT NOT NULL,
    version              TEXT,
    changelog            TEXT,
    PRIMARY KEY (id_modfile),
    FOREIGN KEY (id_mod) REFERENCES mod (id_mod) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE IF NOT EXISTS pack_file (
    id_modfile           INTEGER NOT NULL,
    path                 TEXT NOT NULL,
    path_no_extension    TEXT NOT NULL,
    name                 TEXT NOT NULL,
    extension            TEXT,
    PRIMARY KEY (path, id_modfile),
    FOREIGN KEY (id_modfile) REFERENCES modfile (id_modfile) DEFERRABLE INITIALLY DEFERRED
) STRICT;
