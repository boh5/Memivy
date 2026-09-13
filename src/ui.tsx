import { useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
export type Capture = {
  id: string;
  text: string;
  source_app: string;
  project: string | null;
  session_uri: string | null;
  created_at: number;
};
export type Topic = {
  id: string;
  title: string;
  draft: string;
  preview: string;
  updated_at: number;
};
export type Answer = {
  recollection: string;
  ideas: string;
  sources: string[];
  conclusion: string;
};
export type Turn = {
  id: string;
  topic_id: string;
  question: string;
  answer: Answer | null;
  evidence: (Capture & { truncated: boolean })[];
  status: string;
  error: string | null;
  created_at: number;
};
export type Receipt = {
  id: string;
  capture_id: string;
  turn_id: string;
  title: string;
  undone: boolean;
};
export type Thread = { topic: Topic; turns: Turn[]; receipts: Receipt[] };
export type View = {
  topicId: string | null;
  mode: "capture" | "ask";
  draft: string;
  pinned: string[];
};
export type Settings = {
  base_url: string;
  model: string;
  has_key: boolean;
  configured: boolean;
  local: boolean;
  disable_reasoning: boolean;
};
export const emptyView: View = {
  topicId: null,
  mode: "capture",
  draft: "",
  pinned: [],
};
export const emptySettings: Settings = {
  base_url: "",
  model: "",
  has_key: false,
  configured: false,
  local: false,
  disable_reasoning: false,
};
export const native = isTauri();
export const fail = (e: unknown) =>
  typeof e === "string" ? e : "操作未完成，内容仍保留。";
export const date = (n: number) =>
  new Date(n).toLocaleDateString("zh-CN", { month: "long", day: "numeric" });
const previewNotes: Capture[] = [
  {
    id: "preview-note",
    text: "第一次打开产品，应该先让用户看到它能帮自己做什么，再解释配置。",
    source_app: "浏览器布局示例",
    project: null,
    session_uri: null,
    created_at: Date.now(),
  },
];
export async function call<T>(
  name: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (native) return invoke<T>(name, args);
  const reads: Record<string, unknown> = {
    view_state: emptyView,
    workspace_topics: [],
    capture_search: { items: previewNotes },
    model_settings: emptySettings,
    capture_context: "浏览器预览",
  };
  if (name === "memory_source") return previewNotes[0] as T;
  if (name in reads) return reads[name] as T;
  throw "此处仅供预览，请在 Memivy 桌面应用中操作。";
}
const paths: Record<string, string> = {
  plus: "M12 5v14M5 12h14",
  mic: "M9 5a3 3 0 0 1 6 0v7a3 3 0 0 1-6 0zM5 10v2a7 7 0 0 0 14 0v-2M12 19v3M8 22h8",
  download: "M12 3v12m-4-4 4 4 4-4M4 17v4h16v-4",
  play: "m8 4 12 8-12 8z",
  arrow: "M12 19V5m-6 6 6-6 6 6",
  chevron: "m9 5 7 7-7 7",
  close: "m6 6 12 12M6 18 18 6",
  desktop: "M3 4h18v13H3zM8 21h8M12 17v4",
  database: "M21 5c0 2-4 3-9 3S3 7 3 5s4-3 9-3 9 1 9 3ZM3 5v14c0 2 4 3 9 3s9-1 9-3V5M3 12c0 2 4 3 9 3s9-1 9-3",
  link: "M10 13a5 5 0 0 0 7 0l3-3a5 5 0 0 0-7-7l-2 2M14 11a5 5 0 0 0-7 0l-3 3a5 5 0 0 0 7 7l2-2",
  lock: "M5 10h14v11H5zM8 10V6a4 4 0 0 1 8 0v4M12 14v3",
  back: "m14 5-7 7 7 7",
  search: "M21 21l-5-5M18 10a8 8 0 1 1-16 0 8 8 0 0 1 16 0",
  book: "M4 4h6c2 0 2 2 2 2s0-2 2-2h6v16h-6c-2 0-2 1-2 1s0-1-2-1H4zM12 6v15",
  pencil: "m15 4 5 5M4 20l5-1L20 8a2 2 0 0 0-5-5L4 14z",
  wand: "m5 20 12-12-3-3L2 17zM11 8l3 3M19 2v4M17 4h4M20 13v4M18 15h4M7 2v4M5 4h4",
  ellipsis: "M5 12h.01M12 12h.01M19 12h.01",
  chat: "M5 4h14a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H9l-6 4V6a2 2 0 0 1 2-2z",
  spark: "m12 3 2.6 6.4L21 12l-6.4 2.6L12 21l-2.6-6.4L3 12l6.4-2.6z",
  settings:
    "M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8M12 2v3m0 14v3M2 12h3m14 0h3M5 5l2 2m10 10 2 2M5 19l2-2M17 7l2-2",
  expand: "M14 3h7v7M21 3l-8 8M10 21H3v-7m0 7 8-8",
  folder: "M3 6h7l2 2h9v12H3zM3 6V4h7l2 2h9v2",
  pin: "m8 3 8 0-1 7 3 3v2H6v-2l3-3zM12 15v7",
  check: "m5 12 4 4L19 6",
  undo: "M8 4 3 9l5 5M3 9h11a6 6 0 0 1 0 12",
  stop: "M6 6h12v12H6z",
  leaf: "M19 3C6 2 2 8 5 15c4 7 16 2 14-12ZM5 20 15 8",
  history: "M3 11a9 9 0 1 1 2 7M3 4v7h7M12 7v5l3 2",
  refresh: "M3 10a9 9 0 0 1 15-5l3 3M21 3v5h-5M21 14a9 9 0 0 1-15 5l-3-3M3 21v-5h5",
  trash: "M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7M14 10v7",
  note: "M5 3h10l4 4v14H5zM14 3v5h5M9 12h6M9 16h6",
};
export function Icon({ name, size = 18 }: { name: string; size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.65"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d={paths[name] || paths.spark} />
    </svg>
  );
}
export function IconButton({
  label,
  name,
  onClick,
  active = false,
}: {
  label: string;
  name: string;
  onClick: () => void;
  active?: boolean;
}) {
  return (
    <button
      className={`icon-button ${active ? "active" : ""}`}
      title={label}
      aria-label={label}
      onClick={onClick}
    >
      <Icon name={name} />
    </button>
  );
}
export function MemoryCard({
  memory: m,
  onOpen,
  onThink,
}: {
  memory: Capture;
  onOpen: () => void;
  onThink: () => void;
}) {
  return (
    <article
      className={`memory-card ${m.source_app.includes("用户确认") ? "conclusion-memory" : ""}`}
    >
      <button className="memory-card-content" onClick={onOpen}>
        <span className="eyebrow">
          {m.source_app.includes("用户确认") ? "已确认的结论" : "留下的原话"}
          <span>{date(m.created_at)}</span>
        </span>
        {m.project && <h3>{m.project}</h3>}
        <p>{m.text}</p>
      </button>
      <footer>
        <span>{m.source_app.replace("Memivy Phase 1", "来自 Memivy")}</span>
        <button onClick={onThink}>
          接着想 <Icon name="spark" size={13} />
        </button>
      </footer>
    </article>
  );
}
export function Conclusion({
  turn,
  receipts,
  latest,
  onChange,
  onError,
}: {
  turn: Turn;
  receipts: Receipt[];
  latest: boolean;
  onChange: () => void;
  onError: (s: string) => void;
}) {
  const [editing, setEditing] = useState(false),
    [text, setText] = useState(turn.answer?.conclusion || ""),
    [title, setTitle] = useState(turn.question.slice(0, 28)),
    [busy, setBusy] = useState(false);
  const lock = useRef(false),
    request = useRef(crypto.randomUUID());
  const receipt = receipts.find((r) => !r.undone);
  const save = async () => {
    if (lock.current || !text.trim() || !title.trim()) return;
    lock.current = true;
    setBusy(true);
    try {
      await call("confirm_conclusion", {
        requestId: request.current,
        turnId: turn.id,
        title,
        text,
      });
      setEditing(false);
      onChange();
    } catch (e) {
      onError(fail(e));
    } finally {
      lock.current = false;
      setBusy(false);
    }
  };
  const undo = async () => {
    if (!receipt || lock.current) return;
    lock.current = true;
    try {
      await call("undo_conclusion", { id: receipt.id });
      request.current = crypto.randomUUID();
      onChange();
    } catch (e) {
      onError(fail(e));
    } finally {
      lock.current = false;
    }
  };
  if (receipt)
    return (
      <div className="saved-receipt">
        <Icon name="check" size={15} />
        <span>已新增记忆「{receipt.title}」</span>
        <button onClick={() => void undo()}>撤销</button>
      </div>
    );
  if (!latest && !editing)
    return (
      <button
        className="text-button save-older"
        onClick={() => setEditing(true)}
      >
        <Icon name="plus" size={13} />
        留下这段结论
      </button>
    );
  return (
    <div className="conclusion-card">
      <div className="conclusion-top">
        <span>
          <Icon name="leaf" size={15} />
          {receipts.some((r) => r.undone)
            ? "已撤销保存 · 讨论仍保留"
            : "值得留下的一点"}
        </span>
        <span className="draft-label">待你确认</span>
      </div>
      {editing ? (
        <>
          <label className="field-label">
            存为一条新记忆
            <input
              aria-label="新记忆名称"
              maxLength={60}
              value={title}
              onChange={(e) => {
                setTitle(e.target.value);
                request.current = crypto.randomUUID();
              }}
            />
          </label>
          <textarea
            aria-label="确认保存的结论"
            value={text}
            onChange={(e) => {
              setText(e.target.value);
              request.current = crypto.randomUUID();
            }}
          />
          <div className="conclusion-actions">
            <button className="text-button" onClick={() => setEditing(false)}>
              先不存
            </button>
            <button
              className="send-button"
              disabled={busy || !text.trim() || !title.trim()}
              onClick={() => void save()}
            >
              {busy ? "保存中…" : "确认存入记忆"}
              <Icon name="check" size={14} />
            </button>
          </div>
        </>
      ) : (
        <>
          <p>{text}</p>
          <button className="text-button" onClick={() => setEditing(true)}>
            检查并存下 <Icon name="arrow" size={14} />
          </button>
        </>
      )}
    </div>
  );
}
export function SettingsPanel({
  initial,
  onClose,
  onSaved,
}: {
  initial: Settings;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [base, setBase] = useState(initial.base_url),
    [model, setModel] = useState(initial.model),
    [key, setKey] = useState(""),
    [clearKey, setClearKey] = useState(false),
    [quick, setQuick] = useState(initial.disable_reasoning),
    [busy, setBusy] = useState(false),
    [message, setMessage] = useState(""),
    [ok, setOk] = useState(false);
  const [mcp, setMcp] = useState(false);
  useEffect(() => {
    if (native)
      void call<{ mcp_enabled: boolean }>("diagnostics")
        .then((d) => setMcp(d.mcp_enabled))
        .catch((e) => setMessage(fail(e)));
    const previous = document.activeElement;
    return () => {
      if (previous instanceof HTMLElement) previous.focus();
    };
  }, []);
  const toggleMcp = async () => {
    try {
      await call("set_mcp_enabled", { enabled: !mcp });
      setMcp(!mcp);
    } catch (e) {
      setMessage(fail(e));
    }
  };
  const save = async () => {
    if (busy) return;
    setBusy(true);
    setMessage("");
    setOk(false);
    try {
      await call("save_model_settings", {
        baseUrl: base.trim(),
        model: model.trim(),
        apiKey: clearKey ? "" : key || null,
        disableReasoning: quick,
      });
      onSaved();
      setKey("");
      setMessage("配置已保存，正在测试连接…");
      await call("test_model");
      setOk(true);
      setMessage("连接成功，可以回到记忆里提问了。");
    } catch (e) {
      setMessage(fail(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div
      className="modal-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget && !busy) onClose();
      }}
    >
      <section
        className="settings-panel"
        role="dialog"
        aria-modal="true"
        aria-label="模型与设置"
        onKeyDown={(e) => {
          if (e.key !== "Tab") return;
          const items = Array.from(
            e.currentTarget.querySelectorAll<HTMLElement>(
              "button:not(:disabled),input:not(:disabled)",
            ),
          );
          const first = items[0],
            last = items[items.length - 1];
          if (e.shiftKey && document.activeElement === first) {
            e.preventDefault();
            last?.focus();
          } else if (!e.shiftKey && document.activeElement === last) {
            e.preventDefault();
            first?.focus();
          }
        }}
      >
        <div className="drawer-head">
          <span>
            <Icon name="settings" />
            模型与设置
          </span>
          <IconButton name="close" label="关闭设置" onClick={onClose} />
        </div>
        <h2>连接你自己的模型</h2>
        <p>支持 OpenAI-compatible 接口。记录和关键词搜索始终可以在本地使用。</p>
        <label className="field-label">
          API Base URL
          <input
            autoFocus
            placeholder="http://127.0.0.1:11435/v1"
            value={base}
            onChange={(e) => setBase(e.target.value)}
            spellCheck={false}
          />
        </label>
        <label className="field-label">
          模型 ID
          <input
            placeholder="填写端点提供的模型 ID"
            value={model}
            onChange={(e) => setModel(e.target.value)}
            spellCheck={false}
          />
        </label>
        <label className="field-label">
          API Key
          <input
            type="password"
            autoComplete="off"
            placeholder={
              initial.has_key ? "已保存密钥，留空继续使用" : "本地模型可以不填"
            }
            value={key}
            onChange={(e) => setKey(e.target.value)}
          />
        </label>
        {initial.has_key && (
          <label className="checkbox-label">
            <input
              type="checkbox"
              checked={clearKey}
              onChange={(e) => setClearKey(e.target.checked)}
            />
            清除已保存的密钥
          </label>
        )}
        <label className="checkbox-label">
          <input
            type="checkbox"
            checked={quick}
            onChange={(e) => setQuick(e.target.checked)}
          />
          快速回答（端点需支持关闭额外推理）
        </label>
        <div className="settings-note">
          <Icon name="book" size={16} />
          <p>
            问答会向此端点发送当前问题、少量最近讨论和最多 8
            条相关记忆。密钥单独保存在本机配置文件，不进入记忆数据库。
          </p>
        </div>
        {message && (
          <div
            className={ok ? "success-note" : "settings-message"}
            role="status"
          >
            {message}
          </div>
        )}
        <button
          className="send-button settings-save"
          disabled={busy || !base.trim() || !model.trim()}
          onClick={() => void save()}
        >
          {busy ? "正在测试…" : "保存并测试连接"}
          <Icon name="arrow" size={16} />
        </button>
        <div className="settings-footer">
          <button className="text-button" onClick={() => void toggleMcp()}>
            外部 AI 保存入口（MCP）：{mcp ? "已开启" : "已关闭"}
          </button>
          <span>桌面助手可以拖动、固定输入条或收起。</span>
          <button
            className="text-button"
            onClick={() =>
              void call("companion_hide")
                .then(() => setMessage("助手已隐藏，可用 ⌃⌥ M 再次唤起。"))
                .catch((e) => setMessage(fail(e)))
            }
          >
            隐藏桌面助手
          </button>
        </div>
      </section>
    </div>
  );
}
