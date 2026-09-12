CREATE TABLE users(id TEXT PRIMARY KEY, name TEXT NOT NULL, password_hash TEXT NOT NULL);
CREATE TABLE workspaces(id TEXT PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE memberships(user_id TEXT REFERENCES users(id), workspace_id TEXT REFERENCES workspaces(id), role TEXT NOT NULL CHECK(role IN ('viewer','maintainer')), PRIMARY KEY(user_id,workspace_id));
CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
