import { useEffect, useState } from "react";
import { call, errorText, type Settings } from "./api";
import { ErrorNotice, Modal } from "./components";
import DesktopSettings from "./DesktopSettings";
export default function SettingsPanel({
  onClose,
  onChanged,
}: {
  onClose: () => void;
  onChanged: () => void;
}) {
  const [settings, setSettings] = useState<Settings>({
    configured: false,
    base_url: "",
    model: "",
    has_key: false,
    disable_reasoning: false,
  });
  const [key, setKey] = useState(""),
    [clearKey, setClearKey] = useState(false),
    [error, setError] = useState(""),
    [notice, setNotice] = useState(""),
    [busy, setBusy] = useState(false),
    [rebuild, setRebuild] = useState(false),
    [ready, setReady] = useState(false),
    [unreadable, setUnreadable] = useState(false);
  useEffect(() => {
    void call<Settings>("workspace_settings")
      .then((s) => {
        setSettings(s);
        setReady(true);
      })
      .catch((e) => {
        setError(errorText(e));
        setUnreadable(true);
        setReady(true);
      });
  }, []);
  async function run(work: () => Promise<string>) {
    if (busy) return;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      setNotice(await work());
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  }
  async function save() {
    await call("workspace_configure", {
      baseUrl: settings.base_url.trim(),
      model: settings.model.trim(),
      apiKey: clearKey ? "" : key || null,
      disableReasoning: settings.disable_reasoning,
      replaceUnreadable: unreadable,
    });
    setKey("");
    setClearKey(false);
    setUnreadable(false);
    setSettings(await call<Settings>("workspace_settings"));
    onChanged();
  }
  return (
    <Modal
      title="设置"
      onClose={() => {
        if (!busy) onClose();
      }}
    >
      <DesktopSettings />
      <section className="settings-section">
        <h3>本地数据</h3>
        <p>记忆和草稿保存在这台 Mac。记录、编辑、搜索和导出都不依赖模型。</p>
        <div className="setting-line">
          <div>
            <strong>修复搜索索引</strong>
            <p>从本地记录重建索引，记忆正文保持原样。</p>
          </div>
          <button
            className="outline-button"
            disabled={busy}
            onClick={() => setRebuild(true)}
          >
            重建索引
          </button>
        </div>
      </section>
      <section className="settings-section">
        <h3>自己的模型</h3>
        <p>
          连接后即可问一问、接着讨论。提问会发送问题、必要的近期对话和相关记忆节选到这个模型。
        </p>
        {unreadable && (
          <p className="workspace-warning">
            现有模型配置无法读取。重新填写并保存会替换这份配置；本地记录仍可使用。
          </p>
        )}
        <label>
          API 地址
          <input
            type="url"
            autoComplete="off"
            aria-label="API 地址"
            placeholder="https://example.com/v1"
            value={settings.base_url}
            disabled={busy || !ready}
            onChange={(e) =>
              setSettings((s) => ({ ...s, base_url: e.target.value }))
            }
          />
        </label>
        <label>
          模型名称
          <input
            aria-label="模型名称"
            autoComplete="off"
            value={settings.model}
            disabled={busy || !ready}
            onChange={(e) =>
              setSettings((s) => ({ ...s, model: e.target.value }))
            }
          />
        </label>
        <label>
          API Key
          <input
            type="password"
            autoComplete="new-password"
            aria-label="API Key"
            placeholder={
              settings.has_key ? "已保存，留空保留" : "本地模型可以留空"
            }
            value={key}
            disabled={busy || !ready || clearKey}
            onChange={(e) => setKey(e.target.value)}
          />
        </label>
        {settings.has_key && (
          <label className="checkbox-label">
            <input
              type="checkbox"
              checked={clearKey}
              disabled={busy}
              onChange={(e) => setClearKey(e.target.checked)}
            />
            清除已保存的 Key
          </label>
        )}
        <label className="checkbox-label">
          <input
            type="checkbox"
            checked={settings.disable_reasoning}
            disabled={busy || !ready}
            onChange={(e) =>
              setSettings((s) => ({
                ...s,
                disable_reasoning: e.target.checked,
              }))
            }
          />
          快速响应（关闭模型推理，端点需支持）
        </label>
        <p className="field-help">
          Key 单独保存在本机配置文件中。更换 API 地址时需要重新填写
          Key。连接测试只发送一段固定测试文字。
        </p>
        <div className="action-row">
          <button
            className="send-button"
            disabled={
              busy ||
              !ready ||
              !settings.base_url.trim() ||
              !settings.model.trim()
            }
            onClick={() =>
              void run(async () => {
                await save();
                return "模型配置已保存在本机";
              })
            }
          >
            保存配置
          </button>
          <button
            className="outline-button"
            disabled={
              busy ||
              !ready ||
              !settings.base_url.trim() ||
              !settings.model.trim()
            }
            onClick={() =>
              void run(async () => {
                await save();
                await call("workspace_test_model");
                return "连接测试通过";
              })
            }
          >
            {busy ? "处理中…" : "保存并测试连接"}
          </button>
        </div>
      </section>
      <section className="settings-section">
        <h3>外部 Agent</h3>
        <p>正式记忆库的 MCP 将在后续版本接通。</p>
      </section>
      <ErrorNotice text={error} />
      {notice && (
        <p className="settings-result" role="status">
          {notice}
        </p>
      )}
      {rebuild && (
        <Modal
          title="重建搜索索引？"
          onClose={() => {
            if (!busy) setRebuild(false);
          }}
        >
          <p>
            索引会从原话和版本重新生成。完成之前请稍候，原始内容不会被修改。
          </p>
          <div className="action-row">
            <button
              className="outline-button"
              disabled={busy}
              onClick={() => setRebuild(false)}
            >
              取消
            </button>
            <button
              className="send-button"
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  await call("library_rebuild");
                  setRebuild(false);
                  onChanged();
                  return "搜索索引已重建";
                })
              }
            >
              {busy ? "重建中…" : "开始重建"}
            </button>
          </div>
          <ErrorNotice text={error} />
        </Modal>
      )}
    </Modal>
  );
}
