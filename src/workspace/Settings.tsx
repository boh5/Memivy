import { useEffect, useRef, useState } from "react";
import { call, errorText, native } from "./api";
import { Icon } from "../ui";
import { Modal, ErrorNotice } from "./components";
import GeneralSettings from "./GeneralSettings";
import BackupSettings from "./BackupSettings";
import McpSettings from "./McpSettings";
import ModelCapability from "./ModelCapability";
import ModelStorage from "./ModelStorage";
import ModelOverview from "./ModelOverview";
import { emptyBinding, modelNames, type Models, type Binding, type Kind, type EmbeddingStatus } from "./modelTypes";
import type { VoiceStatus } from "./useVoice";
import { useTranslation } from "react-i18next";
import { message } from "../i18n/messages";
import { useNotice } from "../i18n/react";
import { invalidateResources } from "./resources";
import "./settings.css";
import logo from "../../design-demo/brand/memivy-icon.svg";

type Page = "general" | "ai" | "desktop" | "data" | "mcp" | Kind;
type StatusKind = "embedding" | "voice";
const tabs = [["general", "settings", "tabs.general"], ["ai", "spark", "tabs.ai"], ["data", "database", "tabs.data"], ["mcp", "link", "tabs.mcp"]] as const;

export default function SettingsPanel({ onClose, onRestore, onChanged, initialPage = "general" }: {
  initialPage?: Page;
  onClose: () => void;
  onRestore: (id: string) => void;
  onChanged: () => void;
}) {
  const { t } = useTranslation("settings");
  const [page, setPage] = useState<Page>(initialPage);
  const [models, setModels] = useState<Models | null>(null);
  const [drafts, setDrafts] = useState<Partial<Record<Kind, Binding>>>({});
  const [loadingModels, setLoadingModels] = useState(true);
  const [modelError, setModelError] = useNotice();
  const [embedding, setEmbedding] = useState<EmbeddingStatus | null>(null);
  const [voice, setVoice] = useState<VoiceStatus | null>(null);
  const [embeddingFailed, setEmbeddingFailed] = useState(false);
  const [voiceFailed, setVoiceFailed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [backupBusy, setBackupBusy] = useState(false);
  const [mcpBusy, setMcpBusy] = useState(false);
  const [repair, setRepair] = useState(false);
  const [repairing, setRepairing] = useState(false);
  const [repaired, setRepaired] = useState(false);
  const [repairError, setRepairError] = useNotice();
  const [notice, setNotice] = useNotice();

  const modelRead = useRef(0);
  const statusReads = useRef({
    embedding: { id: 0, pending: false },
    voice: { id: 0, pending: false },
  });
  const repairLock = useRef(false);
  const locked = busy || backupBusy || mcpBusy || repairing;
  const detailKind = page in modelNames ? page as Kind : null;
  const draft = detailKind && models ? drafts[detailKind] ?? models[detailKind] ?? { ...emptyBinding(), source: "service" } : null;
  const pollsModels = page === "ai" || page === "data" || !!detailKind;

  async function loadModels() {
    const request = ++modelRead.current;
    setLoadingModels(true);
    try {
      const next = await call<Models>("models_load");
      if (request === modelRead.current) { setModels(next); setModelError(""); }
    } catch (e) {
      if (request === modelRead.current) setModelError(errorText(e));
    } finally {
      if (request === modelRead.current) setLoadingModels(false);
    }
  }

  useEffect(() => {
    void loadModels();
    return () => { modelRead.current++; };
  }, []);

  async function readStatus<T>(kind: StatusKind, publish: (value: T) => void, report: (failed: boolean) => void, automatic: boolean) {
    const read = statusReads.current[kind];
    if (automatic && read.pending) return;
    const id = ++read.id;
    read.pending = true;
    try {
      const status = await call<T>(kind === "embedding" ? "embedding_status" : "voice_status");
      if (id === read.id) { publish(status); report(false); }
    } catch {
      if (id === read.id) report(true);
    } finally {
      if (id === read.id) read.pending = false;
    }
  }

  async function refresh(automatic = false) {
    await Promise.all([
      readStatus("embedding", setEmbedding, setEmbeddingFailed, automatic),
      readStatus("voice", setVoice, setVoiceFailed, automatic),
    ]);
  }

  useEffect(() => {
    if (!pollsModels) return;
    void refresh(true);
    const timer = setInterval(() => void refresh(true), 1500);
    return () => {
      clearInterval(timer);
      for (const read of Object.values(statusReads.current)) { read.id++; read.pending = false; }
    };
  }, [pollsModels]);

  useEffect(() => {
    if (!notice) return;
    const timer = setTimeout(() => setNotice(""), 3000);
    return () => clearTimeout(timer);
  }, [notice, setNotice]);

  function navigate(next: Page) {
    if (locked) return;
    setPage(next); setNotice("");
  }

  function saved(next: Models) {
    setModels(next);
    setNotice(message("settings", "status.savedLocally"));
    onChanged();
  }

  async function rebuildSearch() {
    if (repairLock.current) return;
    repairLock.current = true;
    setRepairing(true); setRepairError("");
    try {
      await call("library_rebuild");
      await invalidateResources([{ domain: "search" }]);
      setRepaired(true); setRepair(false);
    } catch (e) {
      setRepairError(errorText(e));
    } finally {
      repairLock.current = false;
      setRepairing(false);
    }
  }

  const active = detailKind ? "ai" : page === "desktop" ? "general" : page;
  const tab = tabs.find(item => item[0] === active);
  const title = detailKind ? t(modelNames[detailKind]) : t(tab?.[2] ?? "title");
  const showEmbeddingFailure = embeddingFailed && (page === "ai" || page === "data" || detailKind === "embedding");
  const showVoiceFailure = voiceFailed && (page === "ai" || page === "data" || detailKind === "voice");

  return <Modal title={t("title")} className="model-settings" onClose={() => { if (!locked) onClose(); }}>
    <div className="settings-shell">
      <aside className="settings-nav">
        <h2><img src={logo} alt="" />{t("title")}</h2>
        <nav aria-label={t("nav.categoryLabel")}>{tabs.map(([id, icon, label]) =>
          <button key={id} className={active === id ? "active" : ""} aria-current={active === id ? "page" : undefined}
            disabled={locked} onClick={() => navigate(id)}><Icon name={icon} />{t(label)}</button>)}</nav>
      </aside>
      <main className="settings-main">
        <header className="model-page-header">
          {detailKind && <button className="icon-button" aria-label={t("actions.backToModels")} disabled={locked}
            onClick={() => navigate("ai")}><Icon name="back" /></button>}
          <div><h2>{title}</h2></div>
        </header>
        {!native && <p className="model-preview-note">{t("preview.readOnly")}</p>}
        {page === "ai" && <div className="settings-page">
          <ModelOverview models={models} embedding={embedding} voice={voice} disabled={locked} onConfigure={navigate} />
        </div>}
        {detailKind && !models && loadingModels && <div className="settings-page"><p role="status">{t("status.readingSettings")}</p></div>}
        {detailKind && models && draft && <ModelCapability key={detailKind} kind={detailKind} models={models} draft={draft}
          setDraft={value => setDrafts(old => ({ ...old, [detailKind]: value }))} embedding={embedding} voice={voice}
          onRefresh={refresh} onModels={saved} onReconcile={setModels} onSaved={next => {
            saved(next); setDrafts(old => ({ ...old, [detailKind]: undefined })); setPage("ai");
          }} onDesktop={() => navigate("desktop")} onBusy={setBusy} />}
        {(page === "general" || page === "desktop") &&
          <GeneralSettings onClose={onClose} focusEntry={page === "desktop"} onBusyChange={setBusy} />}
        {page === "data" && <div className="settings-page">
          <BackupSettings disabled={busy || mcpBusy || repairing} onBusyChange={setBackupBusy} onRestore={onRestore} />
          <ModelStorage models={models} embedding={embedding} voice={voice} onRefresh={refresh} onBusy={setBusy} />
          <details>
            <summary>{t("searchMaintenance.title")}</summary>
            <div className="setting-line">
              <div><strong>{t("searchMaintenance.rebuildTitle")}</strong><p>{t("searchMaintenance.rebuildDescription")}</p></div>
              <button className="outline-button" disabled={locked || !native}
                onClick={() => { setRepairError(""); setRepaired(false); setRepair(true); }}>{t("actions.repair")}</button>
            </div>
            {repaired && <p className="settings-result" role="status">{t("status.searchIndexRepaired")}</p>}
            <button className="model-text-button" disabled={locked} onClick={() => navigate("embedding")}>{t("actions.manageSemanticIndex")} →</button>
          </details>
        </div>}
        {page === "mcp" && <div className="settings-page"><McpSettings onBusyChange={setMcpBusy} /></div>}
        {notice && <p className="model-notice" role="status">{notice}</p>}
        {modelError && pollsModels && <div className="model-error">
          <ErrorNotice text={modelError} />
          <button className="model-text-button" disabled={locked || loadingModels} onClick={() => void loadModels()}>
            {t(loadingModels ? "status.readingSettings" : "actions.reloadSettings")}
          </button>
        </div>}
        {(showEmbeddingFailure || showVoiceFailure) && <div className="model-error">
          {showEmbeddingFailure && <ErrorNotice text={t("status.embeddingReadFailed")} />}
          {showVoiceFailure && <ErrorNotice text={t("status.voiceReadFailed")} />}
          <button className="model-text-button" disabled={locked} onClick={() => {
            if (showEmbeddingFailure) void readStatus("embedding", setEmbedding, setEmbeddingFailed, true);
            if (showVoiceFailure) void readStatus("voice", setVoice, setVoiceFailed, true);
          }}>{t("status.retryModelStatus")}</button>
        </div>}
      </main>
    </div>
    {repair && <Modal title={t("searchMaintenance.dialogTitle")} onClose={() => { if (!repairing) setRepair(false); }}>
      <p>{t("searchMaintenance.dialogDescription")}</p>
      <ErrorNotice text={repairError} />
      {repairing && <p role="status">{t("searchMaintenance.working")}</p>}
      <div className="action-row">
        <button className="outline-button" disabled={repairing} onClick={() => setRepair(false)}>{t("actions.cancel")}</button>
        <button className="send-button" disabled={repairing} onClick={() => void rebuildSearch()}>
          {t(repairError ? "actions.retry" : "searchMaintenance.start")}
        </button>
      </div>
    </Modal>}
  </Modal>;
}
