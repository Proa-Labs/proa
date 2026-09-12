CREATE TABLE activity(id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, issue_id TEXT NOT NULL, user_id TEXT NOT NULL REFERENCES users(id), client_id TEXT NOT NULL, note TEXT NOT NULL, created_at INTEGER NOT NULL, FOREIGN KEY(workspace_id,issue_id) REFERENCES issues(workspace_id,id));
CREATE INDEX activity_issue ON activity(workspace_id,issue_id,created_at);
CREATE TABLE idempotency(user_id TEXT, client_id TEXT, workspace_id TEXT, operation TEXT, key TEXT, input_hash TEXT NOT NULL, result TEXT NOT NULL, PRIMARY KEY(user_id,client_id,workspace_id,operation,key));
