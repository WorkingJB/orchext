import { useEffect, useMemo, useRef, useState } from "react";
import ForceGraph2D from "react-force-graph-2d";
import { api, GraphSnapshot } from "./api";

export function GraphView({
  onSelectDoc,
}: {
  onSelectDoc?: (id: string) => void;
}) {
  const [snap, setSnap] = useState<GraphSnapshot | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const [size, setSize] = useState({ w: 0, h: 0 });

  async function refresh() {
    try {
      setSnap(await api.graphSnapshot());
    } catch (e) {
      setErr(String(e));
    }
  }

  useEffect(() => {
    void refresh();
    let unlisten: (() => void) | undefined;
    api
      .onVaultChanged(() => {
        void refresh();
      })
      .then((fn) => {
        unlisten = fn;
      });
    return () => unlisten?.();
  }, []);

  useEffect(() => {
    if (!containerRef.current) return;
    const el = containerRef.current;
    const ro = new ResizeObserver(() => {
      setSize({ w: el.clientWidth, h: el.clientHeight });
    });
    ro.observe(el);
    setSize({ w: el.clientWidth, h: el.clientHeight });
    return () => ro.disconnect();
  }, []);

  // ForceGraph2D mutates the data objects it receives; give it a fresh
  // shallow copy per snapshot so repeated refreshes don't blow up on
  // pre-existing `x`/`y`/`vx`/`vy` coords from the previous layout.
  const data = useMemo(() => {
    if (!snap) return { nodes: [], links: [] };
    return {
      nodes: snap.nodes.map((n) => ({ ...n })),
      links: snap.edges.map((e) => ({ ...e })),
    };
  }, [snap]);

  return (
    <div className="h-full flex flex-col">
      <div className="px-4 h-10 flex items-center justify-between border-b border-neutral-200 bg-white text-sm">
        <div className="text-neutral-600">
          {snap ? (
            <>
              {snap.nodes.length} node{snap.nodes.length === 1 ? "" : "s"} ·{" "}
              {snap.edges.length} edge{snap.edges.length === 1 ? "" : "s"}
            </>
          ) : (
            "Loading…"
          )}
        </div>
        <button
          onClick={() => void refresh()}
          className="text-xs text-neutral-500 hover:text-neutral-900"
        >
          Refresh
        </button>
      </div>
      {err && (
        <div className="m-4 p-3 bg-red-50 text-red-700 text-sm rounded-lg border border-red-200">
          {err}
        </div>
      )}
      <div ref={containerRef} className="flex-1 min-h-0 bg-neutral-50">
        {snap && snap.nodes.length === 0 && (
          <div className="h-full flex items-center justify-center text-sm text-neutral-500">
            No documents yet — create one to see it here.
          </div>
        )}
        {snap && snap.nodes.length > 0 && size.w > 0 && size.h > 0 && (
          <ForceGraph2D
            graphData={data}
            width={size.w}
            height={size.h}
            nodeId="id"
            nodeLabel={(n: any) => `${n.title}  (${n.type})`}
            nodeColor={(n: any) => colorForType(n.type)}
            nodeRelSize={5}
            linkColor={() => "rgba(120,120,120,0.35)"}
            linkDirectionalArrowLength={3}
            linkDirectionalArrowRelPos={1}
            cooldownTicks={80}
            onNodeClick={(n: any) => {
              onSelectDoc?.(n.id);
            }}
          />
        )}
      </div>
    </div>
  );
}

const TYPE_COLORS: Record<string, string> = {
  identity: "#6366f1",
  roles: "#0ea5e9",
  goals: "#22c55e",
  relationships: "#ec4899",
  memories: "#f59e0b",
  tools: "#14b8a6",
  preferences: "#a855f7",
  domains: "#ef4444",
  decisions: "#3b82f6",
  attachments: "#64748b",
};

function colorForType(t: string): string {
  return TYPE_COLORS[t] ?? "#737373";
}
