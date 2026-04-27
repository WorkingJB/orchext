-- Phase 3 platform Slice 1 follow-up: per-doc author for private-
-- visibility filtering.
--
-- The "My notes for [Org]" UX puts private docs inside the org
-- tenant's vault, where other members would otherwise see them. We
-- need to know who wrote each doc so the read path can filter
-- visibility=private to the author only.
--
-- Existing rows pre-date this column; we leave their author_account_id
-- NULL and treat NULL as "legacy doc, visible to all members of the
-- tenant" so the migration is non-disruptive. New writes set the
-- column to the session's account_id on first INSERT and preserve it
-- on UPDATE (the original author keeps ownership; subsequent edits
-- by other members don't reassign).

ALTER TABLE documents
    ADD COLUMN author_account_id UUID REFERENCES accounts(id) ON DELETE SET NULL;

-- Read-side filter for `visibility = 'private'` joins by author. The
-- index speeds the common shape "list this user's private docs in
-- this tenant".
CREATE INDEX documents_author_idx
    ON documents (tenant_id, author_account_id)
    WHERE author_account_id IS NOT NULL;
