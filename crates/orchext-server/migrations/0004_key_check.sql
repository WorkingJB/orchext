-- Phase 2b.5 hardening: store a key-check blob alongside the wrapped
-- content key so the server can verify that a published session key
-- actually matches the tenant's wrapped one.
--
-- Without this column, `publish_session_key` could only check that the
-- submitted blob decoded to 32 bytes — any authenticated tenant member
-- could push attacker-chosen key bytes and corrupt subsequent
-- ciphertext. The blob here is a sealed marker plaintext sealed under
-- the content key (see `orchext_crypto::make_key_check`); the server
-- holds it but cannot construct or unseal it without the key itself.

ALTER TABLE tenants ADD COLUMN key_check_blob TEXT;
