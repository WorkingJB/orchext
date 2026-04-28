// OAuth 2.1 + PKCE consent screen.
//
// The agent crafts a URL that lands here:
//   /oauth/authorize
//     ?tenant_id=<uuid>
//     &client_label=<display name>
//     &redirect_uri=<encoded http(s) URI>
//     &code_challenge=<43-char base64url SHA-256>
//     &code_challenge_method=S256
//     &scope=<space-separated visibility labels>
//     &state=<agent CSRF, echoed back>
//     &response_type=code            (optional, must be "code" if set)
//     &mode=read|read_propose        (optional, default "read")
//     &ttl_days=<int>                (optional, server clamps to [1,365])
//     &max_docs=<int>                (optional)
//     &max_bytes=<int>               (optional)
//
// On approve we POST `/v1/oauth/authorize` with the user's session,
// receive a one-time `oac_*` code, then 302 the browser back to
// `redirect_uri?code=...&state=...` so the agent's loopback listener
// picks it up. On deny we redirect with `error=access_denied&state=...`
// per RFC 6749 §4.1.2.1.

import { useEffect, useMemo, useState } from "react";
import {
  api,
  ApiFailure,
  Membership,
  VISIBILITIES,
} from "./api";
import { SessionProfile } from "./session";
import { LoginView } from "./LoginView";

type AuthGate =
  | { kind: "bootstrapping" }
  | { kind: "anonymous" }
  | { kind: "authenticated"; profile: SessionProfile };

type ParsedRequest = {
  tenantId: string;
  clientLabel: string;
  redirectUri: string;
  codeChallenge: string;
  codeChallengeMethod: string;
  scope: string[];
  state: string | null;
  mode: "read" | "read_propose";
  ttlDays: number | null;
  maxDocs: number | null;
  maxBytes: number | null;
};

type Parse =
  | { ok: true; req: ParsedRequest }
  | { ok: false; reason: string };

const UUID_RE =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export function ConsentView() {
  const [auth, setAuth] = useState<AuthGate>({ kind: "bootstrapping" });

  useEffect(() => {
    let cancelled = false;
    api
      .me()
      .then((resp) => {
        if (cancelled) return;
        setAuth({
          kind: "authenticated",
          profile: {
            accountId: resp.account.id,
            email: resp.account.email,
            displayName: resp.account.display_name,
          },
        });
      })
      .catch(() => {
        if (!cancelled) setAuth({ kind: "anonymous" });
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (auth.kind === "bootstrapping") {
    return (
      <div className="h-full flex items-center justify-center text-neutral-500 dark:text-neutral-400">
        Loading…
      </div>
    );
  }
  if (auth.kind === "anonymous") {
    // The URL stays at /oauth/authorize across login because LoginView
    // only flips local state — no navigation. Once authed we re-render
    // and pick the params back up.
    return (
      <LoginView
        onAuthenticated={(profile) =>
          setAuth({ kind: "authenticated", profile })
        }
      />
    );
  }
  return <AuthorizedConsent profile={auth.profile} />;
}

function AuthorizedConsent({ profile }: { profile: SessionProfile }) {
  const parsed = useMemo(() => parseRequest(window.location.search), []);
  const [memberships, setMemberships] = useState<Membership[] | null>(null);
  const [loadErr, setLoadErr] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [submitErr, setSubmitErr] = useState<string | null>(null);

  useEffect(() => {
    api
      .tenants()
      .then((r) => setMemberships(r.memberships))
      .catch((e) =>
        setLoadErr(e instanceof ApiFailure ? e.message : String(e))
      );
  }, []);

  if (!parsed.ok) {
    return <FatalCard title="Invalid request" body={parsed.reason} />;
  }
  if (loadErr) {
    return <FatalCard title="Couldn't load your tenants" body={loadErr} />;
  }
  if (memberships === null) {
    return (
      <div className="h-full flex items-center justify-center text-neutral-500 dark:text-neutral-400">
        Loading…
      </div>
    );
  }

  const req = parsed.req;
  const tenant = memberships.find((m) => m.tenant_id === req.tenantId);
  if (!tenant) {
    return (
      <FatalCard
        title="Tenant not available"
        body={`You signed in as ${profile.email}, but you are not a member of the requested tenant. Ask the workspace owner to invite you, or sign in with a different account.`}
      />
    );
  }

  const unknownScope = req.scope.filter(
    (s) => !VISIBILITIES.includes(s as (typeof VISIBILITIES)[number])
  );
  if (unknownScope.length > 0) {
    return (
      <FatalCard
        title="Unknown scope"
        body={`Requested scopes are not recognised: ${unknownScope.join(", ")}.`}
      />
    );
  }

  async function approve() {
    setSubmitErr(null);
    setSubmitting(true);
    try {
      const resp = await api.oauthAuthorize({
        tenant_id: req.tenantId,
        client_label: req.clientLabel,
        redirect_uri: req.redirectUri,
        scope: req.scope,
        mode: req.mode,
        code_challenge: req.codeChallenge,
        code_challenge_method: req.codeChallengeMethod,
        ttl_days: req.ttlDays,
        max_docs: req.maxDocs,
        max_bytes: req.maxBytes,
      });
      const url = appendQuery(resp.redirect_uri, {
        code: resp.code,
        state: req.state,
      });
      window.location.assign(url);
    } catch (e) {
      setSubmitErr(e instanceof ApiFailure ? e.message : String(e));
      setSubmitting(false);
    }
  }

  function deny() {
    const url = appendQuery(req.redirectUri, {
      error: "access_denied",
      state: req.state,
    });
    window.location.assign(url);
  }

  const hasPrivate = req.scope.includes("private");

  return (
    <div className="h-full flex items-center justify-center p-6 bg-neutral-50 dark:bg-neutral-900">
      <div className="w-full max-w-lg bg-white dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 rounded-lg shadow-sm p-6">
        <div className="flex items-center justify-between mb-1">
          <h1 className="text-xl font-semibold">Authorize agent</h1>
          <span className="text-xs text-neutral-500 dark:text-neutral-400">{profile.email}</span>
        </div>
        <p className="text-sm text-neutral-600 dark:text-neutral-400 mb-5">
          An agent is requesting a token to act on your behalf in this
          workspace. Review the request before approving.
        </p>

        <Field label="Agent">
          <div className="font-medium">{req.clientLabel}</div>
        </Field>

        <Field label="Workspace">
          <div className="font-medium">{tenant.name}</div>
          <div className="text-xs text-neutral-500 dark:text-neutral-400 font-mono">
            {tenant.tenant_id}
          </div>
        </Field>

        <Field label="Access">
          <div className="flex flex-wrap gap-1">
            {req.scope.map((s) => (
              <span
                key={s}
                className={
                  "inline-block px-1.5 py-0.5 rounded text-xs " +
                  (s === "private"
                    ? "bg-red-100 dark:bg-red-900/30 text-red-700 dark:text-red-400"
                    : "bg-neutral-100 dark:bg-neutral-800 text-neutral-700 dark:text-neutral-300")
                }
              >
                {s}
              </span>
            ))}
            <span className="ml-2 text-xs text-neutral-500 dark:text-neutral-400">
              · {req.mode === "read_propose" ? "read + propose" : "read-only"}
            </span>
          </div>
        </Field>

        <Field label="Token validity">
          <div className="text-sm text-neutral-700 dark:text-neutral-300">
            {req.ttlDays === null
              ? "default (90 days)"
              : `${req.ttlDays} day${req.ttlDays === 1 ? "" : "s"}`}
          </div>
        </Field>

        <Field label="Returns to">
          <code className="text-xs text-neutral-700 dark:text-neutral-300 break-all">
            {req.redirectUri}
          </code>
        </Field>

        {hasPrivate && (
          <div className="mt-4 p-3 bg-red-50 dark:bg-red-900/30 text-red-700 dark:text-red-400 text-sm rounded border border-red-200 dark:border-red-800">
            This token will be able to read documents marked{" "}
            <span className="font-mono">private</span>. Only approve if you
            recognise the agent above and trust it with your most sensitive
            content.
          </div>
        )}

        {submitErr && (
          <div className="mt-4 p-3 bg-red-50 dark:bg-red-900/30 text-red-700 dark:text-red-400 text-sm rounded border border-red-200 dark:border-red-800">
            {submitErr}
          </div>
        )}

        <div className="mt-6 flex gap-2 justify-end">
          <button
            onClick={deny}
            disabled={submitting}
            className="px-3 py-1.5 text-sm border border-neutral-300 dark:border-neutral-700 rounded hover:bg-neutral-50 dark:hover:bg-neutral-800 disabled:opacity-50"
          >
            Deny
          </button>
          <button
            onClick={approve}
            disabled={submitting}
            className="px-3 py-1.5 text-sm bg-brand-600 text-white rounded hover:bg-brand-700 disabled:opacity-50"
          >
            {submitting ? "Authorizing…" : "Approve"}
          </button>
        </div>
      </div>
    </div>
  );
}

function Field({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="mb-3">
      <div className="text-xs uppercase tracking-wider text-neutral-500 dark:text-neutral-400 mb-1">
        {label}
      </div>
      {children}
    </div>
  );
}

function FatalCard({ title, body }: { title: string; body: string }) {
  return (
    <div className="h-full flex items-center justify-center p-6 bg-neutral-50 dark:bg-neutral-900">
      <div className="w-full max-w-lg bg-white dark:bg-neutral-900 border border-red-200 dark:border-red-800 rounded-lg shadow-sm p-6">
        <h1 className="text-lg font-semibold text-red-700 dark:text-red-400 mb-2">{title}</h1>
        <p className="text-sm text-neutral-700 dark:text-neutral-300">{body}</p>
      </div>
    </div>
  );
}

function parseRequest(search: string): Parse {
  const p = new URLSearchParams(search);
  const tenantId = (p.get("tenant_id") ?? "").trim();
  if (!UUID_RE.test(tenantId)) {
    return { ok: false, reason: "tenant_id is missing or not a UUID." };
  }
  const clientLabel = (p.get("client_label") ?? "").trim();
  if (!clientLabel || clientLabel.length > 200) {
    return {
      ok: false,
      reason: "client_label is required and must be 1–200 characters.",
    };
  }
  const redirectUri = (p.get("redirect_uri") ?? "").trim();
  if (!isAcceptableRedirect(redirectUri)) {
    return {
      ok: false,
      reason:
        "redirect_uri must be an https URL or a loopback http URL (127.0.0.1, localhost, [::1]).",
    };
  }
  const codeChallenge = (p.get("code_challenge") ?? "").trim();
  if (codeChallenge.length !== 43 || !/^[A-Za-z0-9_-]+$/.test(codeChallenge)) {
    return {
      ok: false,
      reason:
        "code_challenge must be a 43-char base64url SHA-256 hash (no padding).",
    };
  }
  const codeChallengeMethod = (
    p.get("code_challenge_method") ?? "S256"
  ).trim();
  if (codeChallengeMethod !== "S256") {
    return {
      ok: false,
      reason: "code_challenge_method must be S256 (plain is not accepted).",
    };
  }
  const responseType = p.get("response_type");
  if (responseType !== null && responseType !== "code") {
    return {
      ok: false,
      reason: "response_type must be 'code' if provided.",
    };
  }
  const scopeRaw = (p.get("scope") ?? "").trim();
  const scope = scopeRaw.split(/\s+/).filter((s) => s.length > 0);
  if (scope.length === 0) {
    return { ok: false, reason: "scope must not be empty." };
  }
  const modeRaw = (p.get("mode") ?? "read").trim();
  if (modeRaw !== "read" && modeRaw !== "read_propose") {
    return {
      ok: false,
      reason: "mode must be 'read' or 'read_propose' if provided.",
    };
  }
  const ttlDays = parseOptionalInt(p.get("ttl_days"));
  if (ttlDays !== null && (ttlDays < 1 || ttlDays > 365)) {
    return {
      ok: false,
      reason: "ttl_days must be between 1 and 365 if provided.",
    };
  }
  const maxDocs = parseOptionalInt(p.get("max_docs"));
  const maxBytes = parseOptionalInt(p.get("max_bytes"));

  return {
    ok: true,
    req: {
      tenantId,
      clientLabel,
      redirectUri,
      codeChallenge,
      codeChallengeMethod,
      scope,
      state: p.get("state"),
      mode: modeRaw as "read" | "read_propose",
      ttlDays,
      maxDocs,
      maxBytes,
    },
  };
}

function parseOptionalInt(raw: string | null): number | null {
  if (raw === null || raw.trim() === "") return null;
  const n = Number(raw);
  return Number.isInteger(n) ? n : null;
}

function isAcceptableRedirect(uri: string): boolean {
  const lower = uri.toLowerCase();
  if (lower.startsWith("https://")) return true;
  return (
    lower.startsWith("http://127.0.0.1:") ||
    lower.startsWith("http://127.0.0.1/") ||
    lower === "http://127.0.0.1" ||
    lower.startsWith("http://localhost:") ||
    lower.startsWith("http://localhost/") ||
    lower === "http://localhost" ||
    lower.startsWith("http://[::1]:") ||
    lower.startsWith("http://[::1]/") ||
    lower === "http://[::1]"
  );
}

function appendQuery(
  base: string,
  params: Record<string, string | null>
): string {
  const sep = base.includes("?") ? "&" : "?";
  const parts: string[] = [];
  for (const [k, v] of Object.entries(params)) {
    if (v === null) continue;
    parts.push(`${encodeURIComponent(k)}=${encodeURIComponent(v)}`);
  }
  return parts.length === 0 ? base : `${base}${sep}${parts.join("&")}`;
}
