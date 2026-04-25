// Session bootstrap for the web client.
//
// Phase 2b.5 moved the session token off `localStorage` and onto an
// httpOnly `orchext_session` cookie issued by the server. This module
// holds only the *display-only* account profile loaded from
// `/v1/auth/me`; the bearer secret never reaches JS.
//
// CSRF: the server also sets a non-HttpOnly `orchext_csrf` cookie on
// login/signup. `getCsrfToken()` reads that cookie value and the API
// client mirrors it back as `X-Orchext-CSRF` on state-changing
// requests (double-submit pattern).

export type SessionProfile = {
  accountId: string;
  email: string;
  displayName: string;
};

const CSRF_COOKIE = "orchext_csrf";

export function getCsrfToken(): string | null {
  const all = document.cookie.split(";");
  for (const raw of all) {
    const eq = raw.indexOf("=");
    if (eq < 0) continue;
    const name = raw.slice(0, eq).trim();
    if (name === CSRF_COOKIE) {
      return decodeURIComponent(raw.slice(eq + 1).trim());
    }
  }
  return null;
}
