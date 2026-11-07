-- OG Paper hosted: tables added for account deletion and chat safety. The
-- API also makes these itself on first run (src/schema.js), so applying
-- this before or after that is fine. The columns it adds to paper_reports
-- (kind, subject_id, context) are left to src/schema.js: an ALTER here
-- would fail on a database the API already upgraded.

-- Pages each account has chatted on (to find its messages when the account
-- is deleted).
CREATE TABLE IF NOT EXISTS paper_chatters (
  page_id     TEXT NOT NULL,
  user_id     TEXT NOT NULL,
  created_at  INTEGER NOT NULL,
  PRIMARY KEY (page_id, user_id)
);
CREATE INDEX IF NOT EXISTS paper_chatters_user ON paper_chatters(user_id);

-- People an account has blocked: their chat isn't shown to it, and they
-- can't share pages with it or offer it pages.
CREATE TABLE IF NOT EXISTS paper_blocks (
  user_id     TEXT NOT NULL,
  blocked_id  TEXT NOT NULL,
  created_at  INTEGER NOT NULL,
  PRIMARY KEY (user_id, blocked_id)
);
CREATE INDEX IF NOT EXISTS paper_blocks_blocked ON paper_blocks(blocked_id);
