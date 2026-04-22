import { useEffect, useRef, useState } from "react";
import { api, ChatMessage, OnboardingSeedDoc } from "./api";

type Step = "api-key" | "chat" | "review" | "done";

export function OnboardingView({
  onComplete,
}: {
  onComplete: () => void;
}) {
  const [step, setStep] = useState<Step>("chat");
  const [hasKey, setHasKey] = useState<boolean | null>(null);
  const [apiKeyDraft, setApiKeyDraft] = useState("");
  const [savingKey, setSavingKey] = useState(false);
  const [history, setHistory] = useState<ChatMessage[]>([]);
  const [pending, setPending] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [drafts, setDrafts] = useState<OnboardingSeedDoc[]>([]);
  const [saved, setSaved] = useState(0);
  const chatRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    api
      .settingsStatus()
      .then((s) => {
        setHasKey(s.has_api_key);
        setStep(s.has_api_key ? "chat" : "api-key");
      })
      .catch((e) => setErr(String(e)));
  }, []);

  useEffect(() => {
    chatRef.current?.scrollTo({
      top: chatRef.current.scrollHeight,
      behavior: "smooth",
    });
  }, [history, busy]);

  async function saveKey() {
    setErr(null);
    setSavingKey(true);
    try {
      await api.settingsSetApiKey(apiKeyDraft);
      setHasKey(true);
      setApiKeyDraft("");
      setStep("chat");
    } catch (e) {
      setErr(String(e));
    } finally {
      setSavingKey(false);
    }
  }

  async function send() {
    const text = pending.trim();
    if (!text || busy) return;
    setErr(null);
    const next: ChatMessage[] =
      history.length === 0
        ? [{ role: "user", content: text }]
        : [...history, { role: "user", content: text }];
    setHistory(next);
    setPending("");
    setBusy(true);
    try {
      const { reply } = await api.onboardingChat(next);
      setHistory([...next, { role: "assistant", content: reply }]);
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function finalize() {
    setErr(null);
    setBusy(true);
    try {
      const d = await api.onboardingFinalize(history);
      setDrafts(d);
      setStep("review");
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function confirmSave() {
    setErr(null);
    setBusy(true);
    try {
      const n = await api.onboardingSave(drafts);
      setSaved(n);
      setStep("done");
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="h-full flex flex-col bg-white">
      <header className="px-6 h-12 flex items-center justify-between border-b border-neutral-200">
        <div className="text-sm font-semibold text-neutral-900">Onboarding</div>
        <button
          onClick={onComplete}
          className="text-xs text-neutral-500 hover:text-neutral-900"
        >
          Skip for now
        </button>
      </header>

      {err && (
        <div className="m-4 p-3 bg-red-50 text-red-700 text-sm rounded-lg border border-red-200">
          {err}
        </div>
      )}

      {step === "api-key" && (
        <div className="p-6 max-w-xl mx-auto w-full">
          <h2 className="text-lg font-semibold mb-2">
            Anthropic API key
          </h2>
          <p className="text-sm text-neutral-600 mb-4">
            Onboarding uses Claude Haiku 4.5 to draft your initial context
            documents. Paste your Anthropic API key to continue. The key is
            stored in <code>.ourtex/settings.json</code> in this vault.
          </p>
          <input
            type="password"
            value={apiKeyDraft}
            onChange={(e) => setApiKeyDraft(e.target.value)}
            placeholder="sk-ant-..."
            className="w-full px-3 py-2 border border-neutral-300 rounded text-sm font-mono"
          />
          <div className="mt-4 flex justify-end gap-2">
            <button
              onClick={onComplete}
              className="px-3 py-1.5 text-sm text-neutral-600 hover:bg-neutral-100 rounded"
            >
              Skip
            </button>
            <button
              onClick={saveKey}
              disabled={savingKey || !apiKeyDraft.trim()}
              className="px-3 py-1.5 text-sm bg-brand-600 text-white rounded hover:bg-brand-700 disabled:opacity-50"
            >
              {savingKey ? "Saving…" : "Save and continue"}
            </button>
          </div>
        </div>
      )}

      {step === "chat" && hasKey && (
        <>
          <div
            ref={chatRef}
            className="flex-1 overflow-y-auto px-6 py-4 bg-neutral-50"
          >
            {history.length === 0 && (
              <div className="max-w-2xl mx-auto text-sm text-neutral-600 mb-4">
                <p className="mb-2">
                  Hi — I'll ask a few questions to seed your vault. Type
                  anything to start.
                </p>
                <p className="text-xs text-neutral-500">
                  Good first message: "I'm a staff engineer at a fintech
                  startup working on payments infra."
                </p>
              </div>
            )}
            <div className="max-w-2xl mx-auto space-y-3">
              {history.map((m, i) => (
                <Bubble key={i} role={m.role} content={m.content} />
              ))}
              {busy && <Bubble role="assistant" content="…" />}
            </div>
          </div>
          <div className="border-t border-neutral-200 p-4">
            <div className="max-w-2xl mx-auto flex gap-2">
              <textarea
                value={pending}
                onChange={(e) => setPending(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && !e.shiftKey) {
                    e.preventDefault();
                    void send();
                  }
                }}
                rows={2}
                placeholder="Type a reply… (Enter to send)"
                className="flex-1 px-3 py-2 border border-neutral-300 rounded text-sm resize-none"
              />
              <div className="flex flex-col gap-2">
                <button
                  onClick={send}
                  disabled={busy || !pending.trim()}
                  className="px-3 py-1.5 text-sm bg-brand-600 text-white rounded hover:bg-brand-700 disabled:opacity-50"
                >
                  Send
                </button>
                <button
                  onClick={finalize}
                  disabled={busy || history.length < 2}
                  title="Draft seed documents from this conversation"
                  className="px-3 py-1.5 text-sm border border-brand-600 text-brand-700 rounded hover:bg-brand-50 disabled:opacity-50"
                >
                  Finish
                </button>
              </div>
            </div>
          </div>
        </>
      )}

      {step === "review" && (
        <div className="flex-1 overflow-y-auto p-6 bg-neutral-50">
          <div className="max-w-3xl mx-auto">
            <h2 className="text-lg font-semibold mb-2">
              Review seed documents ({drafts.length})
            </h2>
            <p className="text-sm text-neutral-600 mb-4">
              These will be written to your vault. You can edit or delete
              any of them afterwards in the Documents tab.
            </p>
            <div className="space-y-3 mb-4">
              {drafts.map((d, i) => (
                <div
                  key={i}
                  className="bg-white border border-neutral-200 rounded p-3 text-sm"
                >
                  <div className="flex items-center gap-2 mb-1">
                    <span className="font-mono text-xs text-neutral-700">
                      {d.id}
                    </span>
                    <span className="text-xs text-neutral-500">{d.type}</span>
                    <span className="text-xs text-neutral-500">
                      {d.visibility}
                    </span>
                    <button
                      onClick={() =>
                        setDrafts(drafts.filter((_, j) => j !== i))
                      }
                      className="ml-auto text-xs text-red-600 hover:underline"
                    >
                      Remove
                    </button>
                  </div>
                  <pre className="text-xs text-neutral-700 whitespace-pre-wrap font-mono">
                    {d.body}
                  </pre>
                </div>
              ))}
            </div>
            <div className="flex justify-end gap-2">
              <button
                onClick={() => setStep("chat")}
                className="px-3 py-1.5 text-sm text-neutral-600 hover:bg-neutral-100 rounded"
              >
                Back to chat
              </button>
              <button
                onClick={confirmSave}
                disabled={busy || drafts.length === 0}
                className="px-3 py-1.5 text-sm bg-brand-600 text-white rounded hover:bg-brand-700 disabled:opacity-50"
              >
                {busy ? "Saving…" : `Save ${drafts.length} document${drafts.length === 1 ? "" : "s"}`}
              </button>
            </div>
          </div>
        </div>
      )}

      {step === "done" && (
        <div className="flex-1 flex items-center justify-center p-6">
          <div className="max-w-md text-center">
            <div className="text-2xl mb-2">All set</div>
            <p className="text-sm text-neutral-600 mb-6">
              Saved {saved} document{saved === 1 ? "" : "s"} to your vault.
              Open the Documents tab to review or edit them.
            </p>
            <button
              onClick={onComplete}
              className="px-4 py-2 text-sm bg-brand-600 text-white rounded hover:bg-brand-700"
            >
              Continue
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

function Bubble({ role, content }: { role: string; content: string }) {
  const isUser = role === "user";
  return (
    <div className={isUser ? "flex justify-end" : "flex justify-start"}>
      <div
        className={
          "max-w-[85%] px-3 py-2 rounded-lg text-sm whitespace-pre-wrap " +
          (isUser
            ? "bg-brand-600 text-white"
            : "bg-white text-neutral-900 border border-neutral-200")
        }
      >
        {content}
      </div>
    </div>
  );
}
