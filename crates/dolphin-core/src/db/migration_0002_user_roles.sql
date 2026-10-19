-- v2: the user's own cosmetic roles (name, icon, color). The first role sets the user's name color.
CREATE TABLE IF NOT EXISTS user_roles (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    icon       TEXT NOT NULL,
    color      TEXT NOT NULL,
    sort       INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL,
    device_id  TEXT NOT NULL,
    deleted    INTEGER NOT NULL DEFAULT 0
);
