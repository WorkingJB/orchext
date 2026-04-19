import { useEffect, useState } from "react";
import { api, AuditPage } from "./api";

export function AuditView() {
  const [page, setPage] = useState<AuditPage | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function refresh() {
    try {
      setPage(await api.auditList(500));
    } catch (e) {
      setError(String(e));
    }
  }
  useEffect(() => {
    void refresh();
  }, []);

  return (
    <div className="p-6 max-w-5xl mx-auto">
      <div className="flex items-center justify-between mb-4">
        <h2 className="text-lg font-semibold">Audit log</h2>
        <div className="flex items-center gap-3">
          {page && (
            <span
              className={
                "text-xs px-2 py-1 rounded " +
                (page.chain_valid
                  ? "bg-green-100 text-green-700"
                  : "bg-red-100 text-red-700")
              }
            >
              {page.chain_valid ? "chain verified" : "chain broken"}
            </span>
          )}
          <button
            onClick={refresh}
            className="text-xs text-neutral-500 hover:text-neutral-900"
          >
            Refresh
          </button>
        </div>
      </div>

      {error && (
        <div className="mb-4 p-3 bg-red-50 text-red-700 text-sm rounded-lg border border-red-200">
          {error}
        </div>
      )}

      <div className="bg-white border border-neutral-200 rounded-lg overflow-hidden">
        <table className="w-full text-sm">
          <thead className="bg-neutral-50 text-neutral-600 text-left text-xs uppercase tracking-wider">
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
                <td colSpan={7} className="px-3 py-6 text-center text-neutral-500">
                  No audit entries yet. Actions by any MCP client will land here.
                </td>
              </tr>
            )}
            {page?.entries.map((r) => (
              <tr key={r.seq} className="border-t border-neutral-100">
                <td className="px-3 py-2 text-neutral-500 font-mono text-xs">
                  {r.seq}
                </td>
                <td className="px-3 py-2 text-neutral-600 text-xs">
                  {new Date(r.ts).toLocaleString()}
                </td>
                <td className="px-3 py-2 font-mono text-xs text-neutral-700">
                  {r.actor}
                </td>
                <td className="px-3 py-2">{r.action}</td>
                <td className="px-3 py-2 font-mono text-xs text-neutral-600">
                  {r.document_id ?? "—"}
                </td>
                <td className="px-3 py-2">
                  <div className="flex flex-wrap gap-1">
                    {r.scope_used.map((s) => (
                      <span
                        key={s}
                        className="inline-block px-1.5 py-0.5 rounded text-[10px] bg-neutral-100 text-neutral-700"
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
                        ? "bg-green-100 text-green-700"
                        : r.outcome === "denied"
                        ? "bg-amber-100 text-amber-700"
                        : "bg-red-100 text-red-700")
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
