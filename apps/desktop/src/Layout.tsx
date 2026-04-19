import { useCallback, useEffect, useState } from "react";
import { api, VaultInfo } from "./api";
import { DocumentsView } from "./DocumentsView";
import { TokensView } from "./TokensView";
import { AuditView } from "./AuditView";

type View = "documents" | "tokens" | "audit";

type Counts = {
  documents: number;
  tokens: number;
  audit: number;
};

export function Layout({
  vault,
  onSwitch,
}: {
  vault: VaultInfo;
  onSwitch: () => void;
}) {
  const [view, setView] = useState<View>("documents");
  const [counts, setCounts] = useState<Counts>({
    documents: vault.document_count,
    tokens: 0,
    audit: 0,
  });

  const refreshCounts = useCallback(async () => {
    const [docs, tokens, audit] = await Promise.all([
      api.docList().then((l) => l.length),
      api.tokenList().then((l) => l.length),
      api.auditList(1).then((p) => p.total),
    ]);
    setCounts({ documents: docs, tokens, audit });
  }, []);

  useEffect(() => {
    // Refresh counts whenever the view changes or on mount — cheap, and
    // keeps the sidebar honest after edits in any tab.
    void refreshCounts();
  }, [view, refreshCounts]);

  return (
    <div className="h-full flex flex-col">
      <header className="border-b border-neutral-200 bg-white px-4 h-12 flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="font-semibold">Mytex</span>
          <span className="text-neutral-400">·</span>
          <span className="text-sm text-neutral-600 font-mono truncate max-w-[50ch]">
            {vault.root}
          </span>
        </div>
        <button
          onClick={onSwitch}
          className="text-xs text-neutral-500 hover:text-neutral-900"
        >
          Switch vault
        </button>
      </header>
      <div className="flex flex-1 min-h-0">
        <nav className="w-44 border-r border-neutral-200 bg-white p-2 flex flex-col gap-1">
          <NavBtn
            label="Documents"
            count={counts.documents}
            active={view === "documents"}
            onClick={() => setView("documents")}
          />
          <NavBtn
            label="Tokens"
            count={counts.tokens}
            active={view === "tokens"}
            onClick={() => setView("tokens")}
          />
          <NavBtn
            label="Audit"
            count={counts.audit}
            active={view === "audit"}
            onClick={() => setView("audit")}
          />
        </nav>
        <main className="flex-1 min-w-0 bg-neutral-50">
          {view === "documents" && <DocumentsView onMutated={refreshCounts} />}
          {view === "tokens" && <TokensView onMutated={refreshCounts} />}
          {view === "audit" && <AuditView />}
        </main>
      </div>
    </div>
  );
}

function NavBtn({
  label,
  count,
  active,
  onClick,
}: {
  label: string;
  count: number;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className={
        "flex items-center justify-between text-left px-3 py-2 rounded-md text-sm transition " +
        (active
          ? "bg-brand-50 text-brand-700 font-medium"
          : "text-neutral-700 hover:bg-neutral-100")
      }
    >
      <span>{label}</span>
      <span
        className={
          "text-xs px-1.5 py-0.5 rounded " +
          (active ? "bg-white text-brand-700" : "bg-neutral-100 text-neutral-600")
        }
      >
        {count}
      </span>
    </button>
  );
}
