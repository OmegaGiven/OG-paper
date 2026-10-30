-- OG Paper hosted: page directory, sharing and quotas (D1). Every table is
-- prefixed paper_ so it can share the host site's database. User ids are the
-- host site's (stored as text). See docs/HOSTED.md.

-- One row per account that has used OG Paper: its quota and what it keeps.
-- used = Σ bytes of the pages it owns + quota its busy rooms took ahead.
CREATE TABLE paper_accounts (
  user_id     TEXT PRIMARY KEY,
  quota       INTEGER NOT NULL,
  used        INTEGER NOT NULL DEFAULT 0,
  blocked     INTEGER NOT NULL DEFAULT 0,            -- 1: may not keep anything (admin)
  created_at  INTEGER NOT NULL
);

-- A page (one canvas). The room (Durable Object) holds its content; this is
-- what lists, shares and bills it. edit_key / view_key are the canvas's
-- sealing keys, handed only to members of the matching role.
CREATE TABLE paper_pages (
  id          TEXT PRIMARY KEY,                      -- random, 32 hex
  owner_id    TEXT NOT NULL,                         -- billing owner
  name        TEXT NOT NULL,
  canvas      TEXT NOT NULL,                         -- the canvas id inside (32 hex)
  edit_key    TEXT NOT NULL,
  view_key    TEXT NOT NULL,
  bytes       INTEGER NOT NULL DEFAULT 0,            -- what the room keeps (as last reported)
  blocked     INTEGER NOT NULL DEFAULT 0,            -- 1: taken down by an admin
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);
CREATE INDEX paper_pages_owner ON paper_pages(owner_id, updated_at);

-- Who may open a page and how. The billing owner has an 'owner' row too;
-- other 'owner' rows are co-owners. via_link: added by opening that link
-- (revoking the link removes them).
CREATE TABLE paper_members (
  page_id     TEXT NOT NULL REFERENCES paper_pages(id) ON DELETE CASCADE,
  user_id     TEXT NOT NULL,
  role        TEXT NOT NULL CHECK (role IN ('owner', 'editor', 'viewer')),
  via_link    TEXT,
  added_by    TEXT,
  added_at    INTEGER NOT NULL,
  PRIMARY KEY (page_id, user_id)
);
CREATE INDEX paper_members_user ON paper_members(user_id);

-- "Anyone with the link" links. Only a hash of the token is kept.
CREATE TABLE paper_links (
  id          TEXT PRIMARY KEY,                      -- short id shown to the owner
  token_hash  TEXT NOT NULL UNIQUE,
  page_id     TEXT NOT NULL REFERENCES paper_pages(id) ON DELETE CASCADE,
  role        TEXT NOT NULL CHECK (role IN ('editor', 'viewer')),
  created_by  TEXT NOT NULL,
  created_at  INTEGER NOT NULL
);
CREATE INDEX paper_links_page ON paper_links(page_id);

-- Content reports, for the admin.
CREATE TABLE paper_reports (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  page_id     TEXT NOT NULL,
  reporter_id TEXT,
  reason      TEXT NOT NULL,
  created_at  INTEGER NOT NULL,
  resolved_at INTEGER,
  resolution  TEXT
);
CREATE INDEX paper_reports_open ON paper_reports(resolved_at, created_at);
