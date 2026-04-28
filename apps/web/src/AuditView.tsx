import { useEffect, useState } from "react";
import { api, ApiFailure, AuditResponse, Membership } from "./api";

function errMessage(e: unknown): string {
  return e instanceof ApiFailure ? e.message : String(e);
}

export function AuditView({ tenant }: { tenant: Membership }) {
  const [page, setPage] = useState<AuditResponse | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function refresh() {
    try {
      setError(null);
      setPage(await api.auditList(tenant.tenant_id, 500));
    } catch (e) {
      setError(errMessage(e));
    }
  }
  useEffect(() => {
    setPage(null);
    void refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tenant.tenant_id]);

  return (
    <div className="p-6 max-w-5xl mx-auto">
      <div className="flex items-center justify-between mb-4">
        <h2 className="text-lg font-semibold">Audit log</h2>
        <div className="flex items-center gap-3">
          {page && page.head_hash && (
            <span
              className="text-[10px] font-mono px-2 py-1 rounded bg-neutral-100 dark:bg-neutral-800 text-neutral-600 dark:text-neutral-400"
              title="Current chain head hash"
            >
              head {page.head_hash.slice(0, 12)}…
            </span>
          )}
          <button
            onClick={refresh}
            className="text-xs text-neutral-500 dark:text-neutral-400 hover:text-neutral-900 dark:hover:text-neutral-100"
          >
            Refresh
          </button>
        </div>
      </div>

      {error && (
        <div className="mb-4 p-3 bg-red-50 dark:bg-red-900/30 text-red-700 dark:text-red-400 text-sm rounded-lg border border-red-200 dark:border-red-800">
          {error}
        </div>
      )}

      <div className="bg-white dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 rounded-lg overflow-hidden">
        <table className="w-full text-sm">
          <thead className="bg-neutral-50 dark:bg-neutral-900 text-neutral-600 dark:text-neutral-400 text-left text-xs uppercase tracking-wider">
            <tr>
              <th className="px-3 py-2 w-14">Seq</th>
              <th className="px-3 py-2 w-40">When</th>
              <th className="px-3 py-2 w-40">Actor</th>
              <th className="px-3 py-2">Action</th>
              <th className="px-3 py-2">Document</th>
              <th className="px-3 py-2">Scope</th>
              <th className="px-3 py-2 w-20">Outcome</th>
            </tr>
          </thead>
          <tbody>
            {page && page.entries.length === 0 && (
              <tr>
                <td colSpan={7} className="px-3 py-6 text-center text-neutral-500 dark:text-neutral-400">
                  No audit entries yet. Actions by any MCP client will land here.
                </td>
              </tr>
            )}
            {page?.entries.map((r) => (
              <tr key={r.seq} className="border-t border-neutral-100 dark:border-neutral-800">
                <td className="px-3 py-2 text-neutral-500 dark:text-neutral-400 font-mono text-xs">
                  {r.seq}
                </td>
                <td className="px-3 py-2 text-neutral-600 dark:text-neutral-400 text-xs">
                  {new Date(r.ts).toLocaleString()}
                </td>
                <td className="px-3 py-2 font-mono text-xs text-neutral-700 dark:text-neutral-300">
                  {r.actor}
                </td>
                <td className="px-3 py-2">{r.action}</td>
                <td className="px-3 py-2 font-mono text-xs text-neutral-600 dark:text-neutral-400">
                  {r.document_id ?? "—"}
                </td>
                <td className="px-3 py-2">
                  <div className="flex flex-wrap gap-1">
                    {r.scope_used.map((s) => (
                      <span
                        key={s}
                        className="inline-block px-1.5 py-0.5 rounded text-[10px] bg-neutral-100 dark:bg-neutral-800 text-neutral-700 dark:text-neutral-300"
                      >
                        {s}
                      </span>
                    ))}
                  </div>
                </td>
                <td className="px-3 py-2">
                  <span
                    className={
                      "text-xs px-1.5 py-0.5 rounded " +
                      (r.outcome === "ok"
                        ? "bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400"
                        : r.outcome === "denied"
                        ? "bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-400"
                        : "bg-red-100 dark:bg-red-900/30 text-red-700 dark:text-red-400")
                    }
                  >
                    {r.outcome}
                  </span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
