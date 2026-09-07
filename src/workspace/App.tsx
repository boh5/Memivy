import OrganizationReceipt from "./OrganizationReceipt";
import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import logo from "../../design-demo/brand/memivy-logo.svg";
import { Icon } from "../ui";
import {
  call,
  date,
  errorText,
  keyOf,
  native,
  sourceName,
  type Key,
  type Page,
  type Query,
  type Row,
  type Settings,
  type Topic,
  type Receipt,
  type Detail,
  uid,
} from "./api";
import { Empty, ErrorNotice, Highlight } from "./components";
import { flushDrafts, refreshDrafts } from "./useDraft";
import { installClickRecovery } from "./clickRecovery";
import MemoryDetail from "./MemoryDetail";
import SettingsPanel from "./Settings";
import Discussion from "./Discussion";
import CaptureForm from "./CaptureForm";
import { useDesktop, useWindowLifecycle, type MainRoute } from "./desktopApi";
import "../prototype.css";
import "./workspace.css";
import "./desktop.css";

export default function App() {
  useEffect(() => {
    const root = document.querySelector<HTMLElement>(".formal-app");
    if (native && root) return installClickRecovery(root);
  }, []);
  const [mode, setMode] = useState<"capture" | "ask">("capture");
  const [page, setPage] = useState<"home" | "library" | "trash" | "topic">(
      "home",
    ),
    [query, setQuery] = useState(""),
    [origin, setOrigin] = useState(""),
    [project, setProject] = useState(""),
    [since, setSince] = useState(""),
    [until, setUntil] = useState("");
  const [projects, setProjects] = useState<string[]>([]),
    [recent, setRecent] = useState<Row[]>([]),
    [topics, setTopics] = useState<Topic[]>([]),
    [topic, setTopic] = useState<Topic | null>(null);
  const [selected, setSelected] = useState<Key | null>(null),
    [result, setResult] = useState<Page>({ items: [], next_offset: null }),
    [loading, setLoading] = useState(false),
    [listError, setListError] = useState(""),
    [homeError, setHomeError] = useState("");
  const [pendingReceipt, setPendingReceipt] = useState<{
    key: string;
    receipt: Receipt;
  } | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false),
    [configured, setConfigured] = useState(false),
    [revision, setRevision] = useState(0),
    [focus, setFocus] = useState(0),
    [filtersOpen, setFiltersOpen] = useState(false),
    [savedKey, setSavedKey] = useState<Key | null>(null);
  const refresh = useCallback((key?: Key, receipt?: Receipt) => {
    setRevision((v) => v + 1);
    if (key) setSelected(key);
    setPendingReceipt(key && receipt ? { key: keyOf(key), receipt } : null);
  }, []);
  const desktop = useDesktop();
  const [quick, setQuick] = useState(false), [handoff, setHandoff] = useState<number | null>(null), [windowError, setWindowError] = useState("");
  useWindowLifecycle(setWindowError);
  function handoffReady() {
    if (handoff === null) return;
    const generation = handoff;
    setHandoff(null);
    void call("desktop_handoff_ready", { generation }).catch(e => setWindowError(errorText(e)));
  }
  useEffect(() => {
    if (!native) return;
    const events = [listen("desktop-settings", () => setSettingsOpen(true)), listen<MainRoute>("desktop-route", e => {
      void flushDrafts().then(refreshDrafts).then(() => {
        if (document.querySelector("dialog[open]")) throw "请先完成或关闭主窗口中的对话框，再展开快捷窗口。";
        const r = e.payload;
        setQuick(r.quick); setMode(r.mode); setSavedKey(null); setHandoff(r.generation);
        if (r.settings) { setSettingsOpen(true); setPage("home"); }
        else if (r.record) { setSelected(r.record); setPage("library"); }
        else if (r.mode === "ask" && r.topic) { setTopic(r.topic); setPage("topic"); }
        else setPage("home");
        setFocus(v => v + 1); refresh();
      }).catch(error => { setWindowError(errorText(error)); void call("desktop_handoff_ready", { generation: e.payload.generation, failed: true }); });
    })];
    return () => { events.forEach(x => void x.then(stop => stop())); };
  }, [refresh]);
  useEffect(() => {
    if (handoff !== null && (settingsOpen || page === "library")) {
      const frame = requestAnimationFrame(handoffReady);
      return () => cancelAnimationFrame(frame);
    }
  }, [handoff, settingsOpen, page]);
  const search = useRef<HTMLInputElement>(null),
    sequence = useRef(0),
    pageRef = useRef(page);
  pageRef.current = page;
  useEffect(() => {
    let alive = true;
    void Promise.all([
      call<Page>("library_query", { query: { query: "", limit: 6 } }),
      call<Topic[]>("library_topics"),
      call<string[]>("library_projects"),
    ])
      .then(([r, t, p]) => {
        if (alive) {
          setRecent(r.items);
          setTopics(t);
          setProjects(p);
          setHomeError("");
        }
      })
      .catch((e) => {
        if (alive) setHomeError(errorText(e));
      });
    void call<Settings>("workspace_settings")
      .then((s) => {
        if (alive) setConfigured(s.configured);
      })
      .catch(() => {
        if (alive) setConfigured(false);
      });
    return () => {
      alive = false;
    };
  }, [revision]);
  useEffect(() => {
    if (!native) return;
    const unlisten = listen("library-refresh", () => refresh());
    const close = listen("workspace-close-request", () => {
      if (document.querySelector("dialog[open]")) { setWindowError("请先完成或关闭当前对话框，再关闭主窗口。"); return; }
      void flushDrafts()
        .then(() => call("workspace_close"))
        .catch((e) => {
          setHomeError(errorText(e));
          setListError(errorText(e));
        });
    });
    return () => {
      void unlisten.then((stop) => stop());
      void close.then((stop) => stop());
    };
  }, [refresh]);
  function newCapture() {
    setQuick(false);
    setMode("capture");
    setPage("home");
    setFocus((v) => v + 1);
  }
  async function ask(question: string, id: string) {
    if (!configured) {
      setSettingsOpen(true);
      throw "先连接一个模型，问题草稿会保留。";
    }
    const t = await call<Topic>("discussion_ask", {
      id,
      topicId: id,
      question,
      context: [],
    });
    if (quick) await desktop.update({ topic_id: t.id });
    setTopic(t);
    setPage("topic");
    refresh();
  }
  async function discuss(detail: Detail) {
    const source = detail.current
      ? { kind: "version", id: detail.current.id }
      : { kind: "capture", id: detail.key.id };
    const t = await call<Topic>("discussion_open", {
      id: uid(),
      title: detail.title,
      context: [source],
    });
    setTopic(t);
    setPage("topic");
    refresh();
  }
  function openRecord(key: Key) {
    setSelected(key);
    setPage("library");
  }
  useEffect(() => {
    const keyboard = (e: KeyboardEvent) => {
      if (e.isComposing || document.querySelector("dialog[open]")) return;
      if (e.metaKey && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPage("library");
        requestAnimationFrame(() => search.current?.focus());
      }
      if (e.metaKey && e.key.toLowerCase() === "n") {
        e.preventDefault();
        newCapture();
      }
    };
    window.addEventListener("keydown", keyboard);
    return () => window.removeEventListener("keydown", keyboard);
  }, []);
  const options: Query = {
    query,
    trash: page === "trash",
    origin: origin || undefined,
    project: project || undefined,
    since: since ? new Date(`${since}T00:00:00`).getTime() : undefined,
    until: until
      ? new Date(
          new Date(`${until}T00:00:00`).setDate(
            new Date(`${until}T00:00:00`).getDate() + 1,
          ),
        ).getTime()
      : undefined,
  };
  const signature = JSON.stringify(options);
  useEffect(() => {
    if (page !== "library" && page !== "trash") return;
    const seq = ++sequence.current;
    setLoading(true);
    setListError("");
    const timer = setTimeout(
      () => {
        void call<Page>("library_query", { query: { ...options, limit: 40 } })
          .then((r) => {
            if (sequence.current === seq) setResult(r);
          })
          .catch((e) => {
            if (sequence.current === seq) {
              setListError(errorText(e));
              setResult({ items: [], next_offset: null });
            }
          })
          .finally(() => {
            if (sequence.current === seq) setLoading(false);
          });
      },
      query ? 160 : 0,
    );
    return () => {
      clearTimeout(timer);
      ++sequence.current;
    };
  }, [signature, revision, page]);
  async function more() {
    if (loading || result.next_offset === null) return;
    setLoading(true);
    const seq = sequence.current;
    try {
      const r = await call<Page>("library_query", {
        query: { ...options, offset: result.next_offset, limit: 40 },
      });
      if (seq === sequence.current)
        setResult((old) => ({
          items: [...old.items, ...r.items],
          next_offset: r.next_offset,
        }));
    } catch (e) {
      if (seq === sequence.current) setListError(errorText(e));
    } finally {
      if (seq === sequence.current) setLoading(false);
    }
  }
  function resetFilters() {
    setQuery("");
    setOrigin("");
    setProject("");
    setSince("");
    setUntil("");
  }
  const filtering = !!(query || origin || project || since || until);
  return (
    <div className="app-shell formal-app">
      <aside className="sidebar">
        <div className="brand">
          <img src={logo} alt="Memivy" />
        </div>
        <button className="new-thought" onClick={newCapture}>
          <Icon name="plus" />
          留下一个想法<span>⌘ N</span>
        </button>
        <nav aria-label="主导航">
          <button
            className={page === "home" ? "selected" : ""}
            onClick={() => setPage("home")}
          >
            <Icon name="spark" />
            首页
          </button>
          <button
            className={page === "library" ? "selected" : ""}
            onClick={() => {
              if (page === "trash") {
                setSelected(null);
                resetFilters();
              }
              setPage("library");
            }}
          >
            <Icon name="book" />
            记忆库
          </button>
          <button
            className={page === "trash" ? "selected" : ""}
            onClick={() => {
              setSelected(null);
              resetFilters();
              setPage("trash");
            }}
          >
            <Icon name="trash" />
            回收站
          </button>
        </nav>
        <div className="sidebar-label">最近的话题</div>
        <div className="topic-list">
          {topics.map((t) => (
            <button
              className={
                topic?.id === t.id && page === "topic" ? "selected" : ""
              }
              key={t.id}
              onClick={() => {
                setTopic(t);
                setPage("topic");
              }}
            >
              <Icon name="chat" size={15} />
              <span>{t.title}</span>
            </button>
          ))}
          {!topics.length && (
            <p>
              聊过的话题，
              <br />
              会留在这里。
            </p>
          )}
        </div>
        <div className="sidebar-bottom">
          <button onClick={() => { void flushDrafts().then(async () => {
            if (page === "topic" && topic) await desktop.update({ topic_id: topic.id });
            await call("desktop_open", { mode: page === "topic" ? "ask" : undefined });
          }).catch(e => setWindowError(errorText(e))); }}><Icon name="leaf" /><span>快捷入口</span></button>
          <button onClick={() => setSettingsOpen(true)}>
            <Icon name="settings" />
            <span className="settings-label">
              设置与数据
              <small>{configured ? "已配置模型" : "本地记录可用"}</small>
            </span>
          </button>
          <div className="local-storage-label">
            <i />
            记忆留在这台 Mac
          </div>
        </div>
      </aside>
      <main className="main-workspace">
        <ErrorNotice text={windowError} />
        {!native && (
          <div className="preview-banner">
            浏览器布局预览 · 固定示例；真实保存和编辑请使用 Mac 应用
          </div>
        )}
        <div hidden={page !== "home"} className="home-page">
          <div className="home-heading">
            <span className="eyebrow">
              <span className="sun-dot" />
              给想法一点生长的空间
            </span>
            <h1>今天，想接着想些什么？</h1>
            <p>记住一点过去，打开一点新的思路。</p>
          </div>
          <CaptureForm
            key={`${quick}:${mode}`}
            quick={quick}
            sourceApp={desktop.state?.source_app}
            onReady={handoffReady}
            mode={mode}
            onMode={(next) => {
              setMode(next);
              setFocus((v) => v + 1);
              setSavedKey(null);
            }}
            onAsk={ask}
            focus={page === "home" && !settingsOpen ? focus : 0}
            onEdit={() => setSavedKey(null)}
            onSaved={(key) => {
              setSavedKey(key);
              refresh();
            }}
          />
          {savedKey && (
            <div className="mutation-receipt" role="status">
              <Icon name="check" size={16} />
              <span>已记下原话，可在记忆库找回。</span>
              <button onClick={() => openRecord(savedKey)}>查看记忆</button>
            </div>
          )}
          {savedKey && <OrganizationReceipt key={`${savedKey.kind}:${savedKey.id}`} record={savedKey} revision={revision} onOpen={openRecord} onRefresh={refresh} />}
          <ErrorNotice text={homeError} />
          {!!topics.length && (
            <section className="home-section">
              <div className="section-heading">
                <h2>上次想到这里</h2>
                <span>留在这里的讨论</span>
              </div>
              <div className="resume-list">
                {topics.slice(0, 3).map((t) => (
                  <button
                    key={t.id}
                    onClick={() => {
                      setTopic(t);
                      setPage("topic");
                    }}
                  >
                    <div className="topic-symbol">
                      <Icon name="chat" />
                    </div>
                    <div>
                      <strong>{t.title}</strong>
                      <p>从这条思路继续</p>
                    </div>
                    <span>{date(t.updated_at)}</span>
                    <Icon name="chevron" size={16} />
                  </button>
                ))}
              </div>
            </section>
          )}
          <section className="home-section">
            <div className="section-heading">
              <h2>刚刚留下的</h2>
              <button onClick={() => setPage("library")}>
                全部记忆 <Icon name="chevron" size={13} />
              </button>
            </div>
            {recent.length ? (
              <div className="memory-grid home-memories">
                {recent.slice(0, 3).map((r) => (
                  <article className="memory-card" key={keyOf(r.key)}>
                    <button
                      className="memory-card-content"
                      onClick={() => openRecord(r.key)}
                    >
                      <div className="eyebrow">
                        {r.key.kind === "capture" ? "记下的原话" : "当前记忆"}
                        <span>{date(r.updated_at)}</span>
                      </div>
                      <h3>{r.title}</h3>
                      <p>{r.snippet}</p>
                    </button>
                    <footer>
                      <span>{sourceName(r.origin)}</span>
                      <button onClick={() => openRecord(r.key)}>
                        打开记忆 <Icon name="chevron" size={12} />
                      </button>
                    </footer>
                  </article>
                ))}
              </div>
            ) : (
              <Empty
                title="从一句话开始"
                text="想到什么，就先留下来。无需先建立目录或连接模型。"
              />
            )}
          </section>
        </div>
        {(page === "library" || page === "trash") && (
          <div className={`library-layout ${selected ? "has-selection" : ""}`}>
            <section
              className="library-list-pane"
              aria-label={page === "trash" ? "回收站列表" : "记忆列表"}
            >
              <div className="library-list-heading">
                <div>
                  <span className="eyebrow">
                    {page === "trash"
                      ? "可以恢复，留一份余地"
                      : "留在这里，下次用得上"}
                  </span>
                  <h1>{page === "trash" ? "回收站" : "你的记忆"}</h1>
                </div>
                <button
                  className="icon-button"
                  aria-label="刷新记忆列表"
                  onClick={() => refresh()}
                >
                  <Icon name="refresh" />
                </button>
              </div>
              <div className="library-search">
                <Icon name="search" size={17} />
                <input
                  ref={search}
                  aria-label="搜索记忆"
                  type="search"
                  placeholder="找回一个想法…"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                />
                <kbd>⌘ K</kbd>
              </div>
              <div className="filter-bar">
                <span>{query ? "按匹配程度" : "最近更新"}</span>
                <button
                  aria-expanded={filtersOpen}
                  onClick={() => setFiltersOpen((v) => !v)}
                >
                  筛选{filtering ? " · 已启用" : ""}
                  <Icon name="chevron" size={12} />
                </button>
              </div>
              {filtersOpen && (
                <div className="library-filters">
                  <label>
                    来源
                    <select
                      aria-label="来源筛选"
                      value={origin}
                      onChange={(e) => setOrigin(e.target.value)}
                    >
                      <option value="">全部来源</option>
                      <option value="user">我的记录</option>
                      <option value="agent">来自 Agent</option>
                      <option value="conversation">确认的结论</option>
                    </select>
                  </label>
                  <label>
                    项目
                    <select
                      aria-label="项目筛选"
                      value={project}
                      onChange={(e) => setProject(e.target.value)}
                    >
                      <option value="">全部项目</option>
                      {projects.map((p) => (
                        <option key={p}>{p}</option>
                      ))}
                    </select>
                  </label>
                  <label>
                    更新于
                    <input
                      aria-label="开始日期"
                      type="date"
                      value={since}
                      onChange={(e) => setSince(e.target.value)}
                    />
                  </label>
                  <label>
                    至
                    <input
                      aria-label="结束日期"
                      type="date"
                      value={until}
                      onChange={(e) => setUntil(e.target.value)}
                    />
                  </label>
                  {filtering && (
                    <button onClick={resetFilters}>清除全部条件</button>
                  )}
                </div>
              )}
              <ErrorNotice text={listError} />
              <div
                className={`library-rows ${loading ? "is-loading" : ""}`}
                aria-busy={loading}
              >
                {result.items.map((r) => (
                  <button
                    className={`library-row ${selected && keyOf(r.key) === keyOf(selected) ? "selected" : ""}`}
                    key={keyOf(r.key)}
                    onClick={() => setSelected(r.key)}
                  >
                    <div className="row-title">
                      <strong>
                        <Highlight text={r.title} query={query} />
                      </strong>
                    </div>
                    <p>
                      <Highlight text={r.snippet} query={query} />
                    </p>
                    <div className="row-meta">
                      <span>
                        {r.matched_capture ? "命中原话 · " : ""}
                        {sourceName(r.origin)}
                      </span>
                      <time>{date(r.updated_at)}</time>
                    </div>
                  </button>
                ))}
                {!result.items.length && !loading && !listError && (
                  <Empty
                    title={
                      filtering
                        ? "没有找到匹配的记忆"
                        : page === "trash"
                          ? "回收站是空的"
                          : "从一句话开始"
                    }
                    text={
                      filtering
                        ? "试试更短的词，或清除筛选条件。"
                        : page === "trash"
                          ? "删除的记忆会留在这里，直到你明确永久删除。"
                          : "留下的原话，即使尚未整理也会在这里。"
                    }
                  >
                    {filtering ? (
                      <button className="outline-button" onClick={resetFilters}>
                        清除筛选
                      </button>
                    ) : (
                      page !== "trash" && (
                        <button className="outline-button" onClick={newCapture}>
                          记下一个想法
                        </button>
                      )
                    )}
                  </Empty>
                )}
                {loading && (
                  <p className="list-progress" role="status">
                    正在读取本地记忆…
                  </p>
                )}
                {result.next_offset !== null && (
                  <button
                    className="load-more"
                    disabled={loading}
                    onClick={() => void more()}
                  >
                    加载更多
                  </button>
                )}
              </div>
            </section>
            {selected ? (
              <MemoryDetail
                key={keyOf(selected)}
                record={selected}
                initialReceipt={
                  pendingReceipt?.key === keyOf(selected)
                    ? pendingReceipt.receipt
                    : null
                }
                revision={revision}
                query={query}
                onChanged={refresh}
                onDiscuss={discuss}
                onBack={() => setSelected(null)}
              />
            ) : (
              <section className="memory-detail-pane unselected">
                <Empty
                  title="选一条记忆，接着看"
                  text="当前内容、当时的原话和每次变化，都在这里。"
                />
              </section>
            )}
          </div>
        )}
        {page === "topic" && topic && (
          <Discussion
            key={topic.id}
            topic={topic}
            focus={focus}
            onReady={handoffReady}
            revision={revision}
            configured={configured}
            onSettings={() => setSettingsOpen(true)}
            onRefresh={() => refresh()}
            onOpenRecord={openRecord}
          />
        )}
      </main>
      {settingsOpen && (
        <SettingsPanel
          onClose={() => setSettingsOpen(false)}
          onChanged={() => refresh()}
        />
      )}
    </div>
  );
}
