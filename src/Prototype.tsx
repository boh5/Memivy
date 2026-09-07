import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import logo from "../design-demo/brand/memivy-logo.svg";
import icon from "../design-demo/brand/memivy-icon.svg";
import {
  call,
  native,
  fail,
  date,
  emptyView,
  emptySettings,
  Icon,
  IconButton,
  MemoryCard,
  Conclusion,
  SettingsPanel,
  type Capture,
  type Topic,
  type Turn,
  type Thread,
  type View,
  type Settings,
} from "./ui";
import "./prototype.css";
const floating =
  new URLSearchParams(location.search).get("window") === "capture";
export default function Prototype() {
  const [view, setView] = useState<View>(emptyView),
    [topics, setTopics] = useState<Topic[]>([]),
    [memories, setMemories] = useState<Capture[]>([]),
    [thread, setThread] = useState<Thread | null>(null),
    [settings, setSettings] = useState<Settings>(emptySettings);
  const [page, setPage] = useState<"home" | "library">("home"),
    [query, setQuery] = useState(""),
    [error, setError] = useState(""),
    [notice, setNotice] = useState(""),
    [busy, setBusy] = useState(false),
    [expanded, setExpanded] = useState(!floating),
    [pinnedOpen, setPinnedOpen] = useState(
      localStorage.getItem("memivy-companion-pinned") === "true",
    ),
    [source, setSource] = useState<Capture | null>(null),
    [settingsOpen, setSettingsOpen] = useState(false),
    [revision, setRevision] = useState(0),
    [focusRequest, setFocusRequest] = useState(0),
    [ready, setReady] = useState(false);
  const focusComposer = useCallback(() => setFocusRequest((n) => n + 1), []);
  const viewRef = useRef(view);
  viewRef.current = view;
  const composer = useRef<HTMLTextAreaElement>(null),
    composing = useRef(false),
    collapseLock = useRef(false),
    submitLock = useRef(false),
    requestId = useRef(crypto.randomUUID()),
    bottom = useRef<HTMLDivElement>(null),
    attemptTopic = useRef<string | null>(null);
  const orbGesture = useRef({ x: 0, y: 0, pressed: false, dragged: false });
  const dragQueue = useRef<Promise<unknown>>(Promise.resolve());
  const pending = thread?.turns.find((t) => t.status === "processing");
  const refresh = useCallback(() => setRevision((r) => r + 1), []);
  useEffect(() => {
    let active = true;
    void call<View>("view_state")
      .then((v) => {
        if (active) {
          setView(v);
          setReady(true);
        }
      })
      .catch((e) => setError(fail(e)));
    if (!native)
      return () => {
        active = false;
      };
    const ls = [
      listen("workspace-changed", refresh),
      listen("capture-saved", refresh),
      listen<View>("view-open", (e) => {
        setView(e.payload);
        setPage("home");
        setSource(null);
        setError("");
        focusComposer();
        refresh();
      }),
      listen("capture-open", () => {
        collapseLock.current = false;
        setExpanded(true);
        setSource(null);
        void call<View>("view_state")
          .then(setView)
          .catch((e) => setError(fail(e)));
        focusComposer();
        refresh();
      }),
      listen("companion-collapsed", () => {
        setExpanded(false);
        collapseLock.current = false;
      }),
    ];
    return () => {
      active = false;
      for (const l of ls) void l.then((f) => f());
    };
  }, [refresh, focusComposer]);
  useEffect(() => {
    let active = true;
    void Promise.all([
      call<Topic[]>("workspace_topics"),
      call<{ items: Capture[] }>("capture_search", { query }),
      call<Settings>("model_settings"),
    ])
      .then(([t, m, s]) => {
        if (active) {
          setTopics(t);
          setMemories(m.items);
          setSettings(s);
        }
      })
      .catch((e) => {
        if (active) setError(fail(e));
      });
    return () => {
      active = false;
    };
  }, [revision, query]);
  useEffect(() => {
    let active = true;
    if (!view.topicId) {
      setThread(null);
      return;
    }
    void call<Thread>("workspace_thread", { id: view.topicId })
      .then((t) => {
        if (active) setThread(t);
      })
      .catch((e) => {
        if (active) setError(fail(e));
      });
    return () => {
      active = false;
    };
  }, [view.topicId, revision]);
  useEffect(() => {
    if (pending) {
      const timer = setInterval(refresh, 1600);
      return () => clearInterval(timer);
    }
  }, [pending?.id, refresh]);
  useEffect(() => {
    if (!native || !ready) return;
    const timer = setTimeout(
      () =>
        void call("workspace_draft", { view }).catch((e) => setError(fail(e))),
      220,
    );
    return () => clearTimeout(timer);
  }, [view, ready]);
  useEffect(() => {
    if (floating && expanded)
      void call("companion_resize", {
        answer: !!thread?.turns.length || !!source,
      }).catch((e) => setError(fail(e)));
  }, [expanded, thread?.turns.length, source, revision]);
  useLayoutEffect(() => {
    // Focus after React mounts the destination composer, including the later
    // thread load. Data refreshes and new answer text must not steal focus.
    if (!expanded || settingsOpen || source || page !== "home") return;
    let active = true;
    let frame = 0;
    // WebKit finishes native mouse focus after the React click handler. Focus
    // its native responder first, then the DOM editor on the next frame.
    void (native ? call("focus_editor") : Promise.resolve())
      .then(() => {
        if (active)
          frame = requestAnimationFrame(() =>
            composer.current?.focus({ preventScroll: true }),
          );
      })
      .catch((e) => { if (active) setError(fail(e)); });
    return () => {
      active = false;
      cancelAnimationFrame(frame);
    };
  }, [expanded, focusRequest, thread?.topic.id, settingsOpen, source, page]);
  useEffect(() => {
    bottom.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [thread?.turns.length, pending?.status]);
  const collapse = useCallback(async () => {
    if (collapseLock.current) return;
    collapseLock.current = true;
    try {
      await call("workspace_draft", { view: viewRef.current });
      await call("capture_hide");
      setExpanded(false);
    } catch (e) {
      setError(fail(e));
    } finally {
      collapseLock.current = false;
    }
  }, []);
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (
        e.key !== "Escape" ||
        e.isComposing ||
        composing.current ||
        e.keyCode === 229
      )
        return;
      if (settingsOpen) {
        setSettingsOpen(false);
        return;
      }
      if (source) {
        setSource(null);
        return;
      }
      if (floating && expanded) {
        e.preventDefault();
        void collapse();
      }
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [collapse, expanded, source, settingsOpen]);
  const selectTopic = async (id: string) => {
    try {
      await call("workspace_draft", { view: viewRef.current });
      const t = await call<Thread>("workspace_thread", { id });
      setView({ topicId: id, mode: "ask", draft: t.topic.draft, pinned: [] });
      setThread(t);
      setPage("home");
      setSource(null);
      setError("");
      focusComposer();
    } catch (e) {
      setError(fail(e));
    }
  };
  const newThought = async (mode: "capture" | "ask" = "capture") => {
    await call("workspace_draft", { view: viewRef.current }).catch((e) =>
      setError(fail(e)),
    );
    setView({ ...emptyView, mode });
    setThread(null);
    setPage("home");
    setSource(null);
    setNotice("");
    attemptTopic.current = null;
    requestId.current = crypto.randomUUID();
    focusComposer();
  };
  const openSource = async (id: string) => {
    try {
      setSource(await call<Capture>("memory_source", { id }));
    } catch (e) {
      setError(fail(e));
    }
  };
  const fromMemory = async (c: Capture) => {
    await call("workspace_draft", { view: viewRef.current }).catch((e) =>
      setError(fail(e)),
    );
    setView({ topicId: null, mode: "ask", draft: "", pinned: [c.id] });
    setThread(null);
    setPage("home");
    setSource(null);
    focusComposer();
  };
  const submit = async () => {
    if (
      submitLock.current ||
      composing.current ||
      !view.draft.trim() ||
      (view.mode === "ask" && pending)
    )
      return;
    submitLock.current = true;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      if (view.mode === "capture") {
        await call<Capture>("capture_save", {
          requestId: requestId.current,
          text: view.draft,
          attachSource: false,
        });
        const next = { ...view, draft: "" };
        setView(next);
        viewRef.current = next;
        await call("workspace_draft", { view: next });
        setNotice("已记下原话，可在记忆库找回。");
        requestId.current = crypto.randomUUID();
        refresh();
        if (floating && !pinnedOpen) void collapse();
      } else {
        const topicId =
          view.topicId || attemptTopic.current || crypto.randomUUID();
        attemptTopic.current = topicId;
        await call("ask_memory", {
          id: requestId.current,
          topicId,
          question: view.draft,
          pinned: view.pinned,
        });
        const next = { ...view, topicId, draft: "" };
        setView(next);
        viewRef.current = next;
        await call("workspace_draft", { view: next });
        requestId.current = crypto.randomUUID();
        refresh();
      }
    } catch (e) {
      setError(fail(e));
    } finally {
      submitLock.current = false;
      setBusy(false);
    }
  };
  const openMain = async () => {
    try {
      await call("open_workspace", { view });
    } catch (e) {
      setError(fail(e));
    }
  };
  const retry = (t: Turn) => {
    setView((v) => ({ ...v, mode: "ask", draft: t.question }));
    requestId.current = crypto.randomUUID();
    composer.current?.focus();
  };
  const cancel = async () => {
    if (pending)
      try {
        await call("cancel_answer", { id: pending.id });
        refresh();
      } catch (e) {
        setError(fail(e));
      }
  };
  const chooseMode = (mode: "capture" | "ask") => {
    setView((v) => ({ ...v, mode }));
    requestId.current = crypto.randomUUID();
    composer.current?.focus();
  };
  const queueDrag = (command: string) => {
    dragQueue.current = dragQueue.current
      .then(() => call(command))
      .catch((e) => setError(fail(e)));
  };
  const endDrag = () => {
    if (!orbGesture.current.pressed) return;
    orbGesture.current.pressed = false;
    if (orbGesture.current.dragged) queueDrag("companion_drag");
    queueDrag("companion_drag_end");
  };
  const dragHandlers = {
    onPointerDown: (e: React.PointerEvent<HTMLElement>) => {
      if (e.button !== 0) return;
      if (expanded && (e.target as HTMLElement).closest("button")) return;
      orbGesture.current = {
        x: e.screenX,
        y: e.screenY,
        pressed: true,
        dragged: false,
      };
      e.currentTarget.setPointerCapture(e.pointerId);
      queueDrag("companion_drag_start");
    },
    onPointerMove: (e: React.PointerEvent<HTMLElement>) => {
      const gesture = orbGesture.current;
      if (!gesture.pressed || !(e.buttons & 1)) return;
      if (
        !gesture.dragged &&
        Math.hypot(e.screenX - gesture.x, e.screenY - gesture.y) < 5
      ) return;
      gesture.dragged = true;
      queueDrag("companion_drag");
    },
    onPointerUp: endDrag,
    onPointerCancel: endDrag,
    onLostPointerCapture: endDrag,
  };
  const composerUI = (
    <div className={`composer ${floating ? "compact" : ""}`}>
      <div className="composer-top">
        <div className="segmented" aria-label="输入用途">
          <button
            className={view.mode === "capture" ? "selected" : ""}
            onClick={() => chooseMode("capture")}
          >
            <Icon name="plus" size={15} />
            记一下
          </button>
          <button
            className={view.mode === "ask" ? "selected" : ""}
            onClick={() => chooseMode("ask")}
          >
            <Icon name="spark" size={15} />
            问一问
          </button>
        </div>
        {!floating && <span className="quiet">想法不用整理好再来</span>}
      </div>
      {view.pinned.length > 0 && (
        <div className="context-row">
          <span>
            <Icon name="book" size={13} />
            带着 {view.pinned.length} 条记忆思考
          </span>
          <button
            onClick={() => setView((v) => ({ ...v, pinned: [] }))}
            aria-label="移除所选记忆"
          >
            <Icon name="close" size={13} />
          </button>
        </div>
      )}
      <textarea
        ref={composer}
        aria-label={view.mode === "capture" ? "记下想法" : "向记忆提问"}
        value={view.draft}
        maxLength={8000}
        disabled={busy}
        placeholder={
          view.mode === "capture"
            ? "一句想法，一段刚刚说过的话……"
            : view.topicId
              ? "接着刚才的话题，你还想到了什么？"
              : view.pinned.length
                ? "关于这段记忆，你想接着想什么？"
                : "想找回什么，或有什么想一起想清楚？"
        }
        onChange={(e) => {
          setView((v) => ({ ...v, draft: e.target.value }));
          if (!view.topicId) attemptTopic.current = null;
          requestId.current = crypto.randomUUID();
        }}
        onCompositionStart={() => {
          composing.current = true;
        }}
        onCompositionEnd={() => {
          composing.current = false;
        }}
        onFocus={() => {
          if (floating && native) void call("capture_ready").catch(() => {});
        }}
        onKeyDown={(e) => {
          if (
            e.nativeEvent.isComposing ||
            composing.current ||
            e.keyCode === 229
          )
            return;
          if (e.key === "Enter" && !e.shiftKey) {
            e.preventDefault();
            void submit();
          }
        }}
      />
      <div className="composer-bottom">
        <span className="input-hint">
          {pending
            ? "正在结合记忆思考…"
            : view.mode === "capture"
              ? "原话先保存在本机"
              : settings.configured
                ? `${settings.local ? "本地模型" : "远程模型"} · ${settings.model}`
                : "连接模型后即可提问"}
          <span className="keyboard-hint"> · ⇧ Enter 换行</span>
        </span>
        {pending && view.mode === "ask" ? (
          <button className="send-button stop" onClick={() => void cancel()}>
            <Icon name="stop" size={14} />
            停止
          </button>
        ) : (
          <button
            className="send-button"
            disabled={busy || !view.draft.trim()}
            onClick={() => void submit()}
          >
            <span>
              {busy ? "正在提交" : view.mode === "capture" ? "记下" : "发送"}
            </span>
            <Icon name="arrow" size={16} />
          </button>
        )}
      </div>
    </div>
  );
  const threadUI = thread && (
    <div className="conversation" aria-live="polite">
      {thread.turns.map((t, index) => (
        <div className="turn" key={t.id}>
          <div className="question-row">
            <span className="you-avatar">我</span>
            <div>{t.question}</div>
          </div>
          <div className="assistant-row">
            <img src={icon} alt="Memivy" />
            <div className="answer-body">
              {t.status === "processing" ? (
                <div className="thinking">
                  <span className="thinking-dot" />
                  正在找相关记忆，再陪你往下想
                  <span className="quiet">
                    你可以收起窗口，回答会留在这个话题里。
                  </span>
                </div>
              ) : t.answer ? (
                <>
                  {t.answer.recollection && (
                    <section className="recollection">
                      <div className="eyebrow">
                        <Icon name="book" size={13} />
                        从你的记忆里
                      </div>
                      <p>{t.answer.recollection}</p>
                      <div className="source-chips">
                        {t.answer.sources.map((id, n) => (
                          <button key={id} onClick={() => void openSource(id)}>
                            <span>{n + 1}</span>
                            {date(
                              t.evidence.find((e) => e.id === id)?.created_at ||
                                t.created_at,
                            )}{" "}
                            的原话
                            <Icon name="chevron" size={12} />
                          </button>
                        ))}
                      </div>
                    </section>
                  )}
                  {t.answer.ideas && (
                    <section className="new-ideas">
                      <div className="eyebrow">
                        <Icon name="spark" size={13} />
                        {t.answer.recollection ? "一起往下想" : "新的思考建议"}
                        {!t.evidence.length && (
                          <span className="quiet"> · 未找到相关记忆</span>
                        )}
                      </div>
                      <p>{t.answer.ideas}</p>
                    </section>
                  )}
                  {t.evidence.length > 0 && (
                    <details className="coverage">
                      <summary>本次参考了 {t.evidence.length} 条记忆</summary>
                      {t.evidence.map((e) => (
                        <button
                          key={e.id}
                          onClick={() => void openSource(e.id)}
                        >
                          {e.text.slice(0, 55)}
                          {e.truncated ? "（节选）" : ""}
                          <Icon name="chevron" size={12} />
                        </button>
                      ))}
                    </details>
                  )}
                  {t.answer.conclusion && (
                    <Conclusion
                      turn={t}
                      receipts={thread.receipts.filter(
                        (r) => r.turn_id === t.id,
                      )}
                      latest={index === thread.turns.length - 1}
                      onChange={refresh}
                      onError={setError}
                    />
                  )}
                </>
              ) : (
                <div className="failed-answer">
                  <p>{t.error || "这次回答没有完成。"}</p>
                  <button className="text-button" onClick={() => retry(t)}>
                    重新提问 <Icon name="undo" size={14} />
                  </button>
                </div>
              )}
            </div>
          </div>
        </div>
      ))}
      <div ref={bottom} />
    </div>
  );
  const sourceUI = source && (
    <aside className="source-drawer">
      <div className="drawer-head">
        <span>
          <Icon name="book" />
          记忆原文
        </span>
        <IconButton
          name="close"
          label="关闭原文"
          onClick={() => setSource(null)}
        />
      </div>
      <span className="eyebrow">
        {date(source.created_at)} · {source.source_app}
      </span>
      <h2>{source.project || "当时留下的话"}</h2>
      <p className="source-text">{source.text}</p>
      <button
        className="outline-button"
        onClick={() => void fromMemory(source)}
      >
        <Icon name="spark" />
        带着这段记忆继续想
      </button>
      <p className="source-footnote">
        {source.source_app.includes("用户确认")
          ? "来自你确认保存的讨论结论。"
          : "原话按输入保存。"}
        <br />
        记录可供下次检索使用。
      </p>
    </aside>
  );
  const alerts = (
    <>
      {error && (
        <div className="error-note" role="alert">
          <span>{error}</span>
          <button aria-label="关闭错误提示" onClick={() => setError("")}>
            <Icon name="close" size={14} />
          </button>
        </div>
      )}
      {notice && (
        <div className="success-note" role="status">
          <Icon name="check" size={14} />
          {notice}
        </div>
      )}
    </>
  );
  if (floating) {
    if (!expanded)
      return (
        <div
          className="companion-rest"
          {...dragHandlers}
          onClick={(e) => {
            if (orbGesture.current.dragged && e.detail !== 0) return;
            if (native)
              void call("companion_open").catch((e) => setError(fail(e)));
            else setExpanded(true);
          }}
        >
          <button
            className={`companion-orb ${pending ? "working" : ""}`}
            aria-label="打开 Memivy 桌面助手"
            title="点击输入 · 按住拖动"
          >
            <img src={icon} alt="" draggable={false} />
            {pending && <i />}
          </button>
          <div className="rest-grip" aria-hidden="true">
            <span />
          </div>
        </div>
      );
    return (
      <div className="floating-frame">
        <div className="companion-panel">
          <header className="companion-header" {...dragHandlers}>
            <div className="companion-identity">
              <img src={icon} draggable={false} />
              <div>
                <strong>Memivy</strong>
                <span>{pending ? "正在想…" : "在这里，接住你的想法"}</span>
              </div>
            </div>
            <div className="window-actions">
              <IconButton
                name="pin"
                active={pinnedOpen}
                label={pinnedOpen ? "取消固定输入条" : "固定输入条"}
                onClick={() => {
                  setPinnedOpen(!pinnedOpen);
                  localStorage.setItem(
                    "memivy-companion-pinned",
                    String(!pinnedOpen),
                  );
                }}
              />
              <IconButton
                name="expand"
                label="展开到主窗口"
                onClick={() => void openMain()}
              />
              <button
                className="icon-button"
                title="收起 · Esc"
                aria-label="收起桌面助手"
                onPointerUp={(e) => {
                  if (e.button === 0) void collapse();
                }}
                onClick={() => void collapse()}
              >
                <Icon name="close" />
              </button>
            </div>
          </header>
          {thread && (
            <div className="floating-topic">
              <span>{thread.topic.title}</span>
              <button onClick={() => void newThought("ask")}>
                <Icon name="plus" size={14} />
                新话题
              </button>
            </div>
          )}
          {source
            ? sourceUI
            : thread && <div className="floating-scroll">{threadUI}</div>}
          {alerts}
          {composerUI}
          {!settings.configured && view.mode === "ask" && (
            <button className="connect-inline" onClick={() => void openMain()}>
              到主窗口连接模型 <Icon name="chevron" size={13} />
            </button>
          )}
        </div>
      </div>
    );
  }
  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <img src={logo} alt="Memivy" />
        </div>
        <button className="new-thought" onClick={() => void newThought()}>
          <Icon name="plus" />
          留下一个想法<span>⌃⌥ M</span>
        </button>
        <nav>
          <button
            className={page === "home" && !view.topicId ? "selected" : ""}
            onClick={() => void newThought("ask")}
          >
            <Icon name="spark" />
            接着想
          </button>
          <button
            className={page === "library" ? "selected" : ""}
            onClick={() => setPage("library")}
          >
            <Icon name="book" />
            记忆库<span>{memories.length}</span>
          </button>
        </nav>
        <div className="sidebar-label">最近的话题</div>
        <div className="topic-list">
          {topics.map((t) => (
            <button
              key={t.id}
              className={
                t.id === view.topicId && page === "home" ? "selected" : ""
              }
              onClick={() => void selectTopic(t.id)}
            >
              <Icon name="chat" size={15} />
              <span>{t.title}</span>
            </button>
          ))}
          {!topics.length && (
            <p>
              聊过的话题会留在这里，
              <br />
              下次可以接着想。
            </p>
          )}
        </div>
        <div className="sidebar-bottom">
          <button
            onClick={() =>
              void call("workspace_draft", { view })
                .then(() => call("show_capture"))
                .catch((e) => setError(fail(e)))
            }
          >
            <Icon name="leaf" />
            桌面助手<span>↗</span>
          </button>
          <button onClick={() => setSettingsOpen(true)}>
            <Icon name="settings" />
            模型与设置
          </button>
          <div className="prototype-label">
            <span />
            交互样机 v2 · 独立测试数据
          </div>
        </div>
      </aside>
      <main className="main-workspace">
        <header className="workspace-header">
          <div className="breadcrumb">
            我的记忆 <span>/</span>{" "}
            {page === "library" ? "记忆库" : thread ? "接着想" : "今天"}
          </div>
          <button
            className={`model-badge ${settings.configured ? "connected" : ""}`}
            onClick={() => setSettingsOpen(true)}
          >
            <i />
            {settings.configured
              ? settings.local
                ? "已配置本地模型"
                : "已配置远程模型"
              : "连接模型"}
            <Icon name="chevron" size={12} />
          </button>
        </header>
        {!native && (
          <div className="preview-banner">
            浏览器布局预览 · 示例数据；真实操作请使用 Mac 样机
          </div>
        )}
        {page === "library" ? (
          <div className="library-page">
            <div className="page-heading">
              <div>
                <span className="eyebrow">留在这里，下次用得上</span>
                <h1>你的记忆</h1>
              </div>
              <span className="soft-label">原话与确认的结论</span>
            </div>
            <div className="library-search">
              <Icon name="search" />
              <input
                aria-label="搜索记忆"
                placeholder="输入关键词，找回一段记忆…"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
              />
              <span>本地搜索</span>
            </div>
            {alerts}
            <div className="memory-grid">
              {memories.map((c) => (
                <MemoryCard
                  key={c.id}
                  memory={c}
                  onOpen={() => void openSource(c.id)}
                  onThink={() => void fromMemory(c)}
                />
              ))}
            </div>
            {!memories.length && (
              <div className="empty-state">
                <Icon name="leaf" size={30} />
                <h2>{query ? "没有找到匹配的记忆" : "从一句话开始"}</h2>
                <p>
                  {query
                    ? "换个词试试，搜索不依赖模型。"
                    : "不需要目录或标签，先把想到的留下来。"}
                </p>
              </div>
            )}
          </div>
        ) : thread ? (
          <div className="thread-page">
            <div className="thread-heading">
              <span className="eyebrow">接着上次的思路</span>
              <h1>{thread.topic.title}</h1>
              <p>讨论留在这个话题里。你确认的结论，才会存入记忆。</p>
            </div>
            <div className="thread-scroll">{threadUI}</div>
            <div className="thread-composer">
              {alerts}
              {composerUI}
            </div>
          </div>
        ) : (
          <div className="home-page">
            <div className="home-heading">
              <span className="eyebrow">
                <span className="sun-dot" />
                给想法一点生长的空间
              </span>
              <h1>今天，想接着想些什么？</h1>
              <p>记住一点过去，打开一点新的思路。</p>
            </div>
            {composerUI}
            {alerts}
            {view.mode === "ask" && !view.draft && (
              <div className="suggestions">
                {(view.pinned.length
                  ? ["帮我把这个想法想得更清楚", "这个想法还有什么盲点？"]
                  : ["我之前为什么做这个决定？", "最近有哪些想法值得接着想？"]
                ).map((q) => (
                  <button
                    key={q}
                    onClick={() => {
                      setView((v) => ({ ...v, draft: q }));
                      requestId.current = crypto.randomUUID();
                      composer.current?.focus();
                    }}
                  >
                    {q}
                    <Icon name="chevron" size={12} />
                  </button>
                ))}
              </div>
            )}
            <section className="home-section">
              <div className="section-heading">
                <h2>上次想到这里</h2>
                <span>不用从头说起</span>
              </div>
              {topics.length ? (
                <div className="resume-list">
                  {topics.slice(0, 3).map((t) => (
                    <button key={t.id} onClick={() => void selectTopic(t.id)}>
                      <div className="topic-symbol">
                        <Icon name="chat" />
                      </div>
                      <div>
                        <strong>{t.title}</strong>
                        <p>{t.preview}</p>
                      </div>
                      <span>{date(t.updated_at)}</span>
                      <Icon name="chevron" size={16} />
                    </button>
                  ))}
                </div>
              ) : (
                <div className="first-thought">
                  <div className="first-thought-icon">
                    <Icon name="leaf" size={26} />
                  </div>
                  <div>
                    <h3>先留下一句话，再一起想一想。</h3>
                    <p>不必先积累很多笔记，也不用把想法整理完整。</p>
                  </div>
                </div>
              )}
            </section>
            <section className="home-section">
              <div className="section-heading">
                <h2>刚刚留下的</h2>
                <button onClick={() => setPage("library")}>
                  全部记忆 <Icon name="chevron" size={13} />
                </button>
              </div>
              {memories.length ? (
                <div className="memory-grid home-memories">
                  {memories.slice(0, 3).map((c) => (
                    <MemoryCard
                      key={c.id}
                      memory={c}
                      onOpen={() => void openSource(c.id)}
                      onThink={() => void fromMemory(c)}
                    />
                  ))}
                </div>
              ) : (
                <p className="empty-line">你的第一段记忆，会出现在这里。</p>
              )}
            </section>
          </div>
        )}
      </main>
      {sourceUI}
      {settingsOpen && (
        <SettingsPanel
          initial={settings}
          onClose={() => setSettingsOpen(false)}
          onSaved={refresh}
        />
      )}
    </div>
  );
}
