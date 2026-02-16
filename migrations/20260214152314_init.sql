CREATE TABLE IF NOT EXISTS server_key_package (
  package_id BLOB NOT NULL PRIMARY KEY,
  client_id TEXT NOT NULL,
  package BLOB NOT NULL,
  created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS server_idx_key_package_client_id ON server_key_package (client_id);

CREATE TABLE IF NOT EXISTS server_message (
  message_id BLOB NOT NULL,
  recipient TEXT NOT NULL,
  content BLOB NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (message_id, recipient)
);

CREATE INDEX IF NOT EXISTS server_idx_message_recipient ON server_message (recipient, created_at);

CREATE TABLE IF NOT EXISTS client_user (
  username TEXT NOT NULL PRIMARY KEY,
  signature_private_key BLOB NOT NULL,
  credential_with_key BLOB NOT NULL
);
