import { invoke } from "@tauri-apps/api/core";

export type VaultInfo = {
  root: string;
  document_count: number;
};

export type DocListItem = {
  id: string;
  type: string;
  title: string;
  visibility: string;
  tags: string[];
  updated: string | null;
};

export type DocDetail = {
  id: string;
  type: string;
  visibility: string;
  tags: string[];
  links: string[];
  aliases: string[];
  source: string | null;
  created: string | null;
  updated: string | null;
  body: string;
  version: string;
};

export type DocInput = {
  id: string;
  type: string;
  visibility: string;
  tags?: string[];
  links?: string[];
  aliases?: string[];
  source?: string | null;
  body: string;
};

export type TokenInfo = {
  id: string;
  label: string;
  scope: string[];
  mode: "read" | "read_propose";
  created_at: string;
  expires_at: string;
  last_used: string | null;
  revoked: boolean;
};

export type IssuedToken = {
  info: TokenInfo;
  secret: string;
};

export type TokenIssueInput = {
  label: string;
  scope: string[];
  mode: "read" | "read_propose";
  ttl_days: number | null;
};

export type AuditRow = {
  seq: number;
  ts: string;
  actor: string;
  action: string;
  document_id: string | null;
  scope_used: string[];
  outcome: string;
};

export type AuditPage = {
  entries: AuditRow[];
  total: number;
  chain_valid: boolean;
};

export const api = {
  vaultOpen: (path: string) => invoke<VaultInfo>("vault_open", { path }),
  vaultInfo: () => invoke<VaultInfo | null>("vault_info"),
  docList: () => invoke<DocListItem[]>("doc_list"),
  docRead: (id: string) => invoke<DocDetail>("doc_read", { id }),
  docWrite: (input: DocInput) => invoke<DocDetail>("doc_write", { input }),
  docDelete: (id: string) => invoke<void>("doc_delete", { id }),
  tokenList: () => invoke<TokenInfo[]>("token_list"),
  tokenIssue: (input: TokenIssueInput) =>
    invoke<IssuedToken>("token_issue", { input }),
  tokenRevoke: (id: string) => invoke<void>("token_revoke", { id }),
  auditList: (limit?: number) =>
    invoke<AuditPage>("audit_list", { limit: limit ?? null }),
};

export const VISIBILITIES = ["public", "work", "personal", "private"] as const;

export const SEED_TYPES = [
  "identity",
  "roles",
  "goals",
  "relationships",
  "memories",
  "tools",
  "preferences",
  "domains",
  "decisions",
] as const;
