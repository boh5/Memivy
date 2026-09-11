import { useEffect, useState } from "react";
import { call, errorText, type Settings } from "./api";
import { ErrorNotice, Modal } from "./components";
import DesktopSettings from "./DesktopSettings";
import BackupSettings from "./BackupSettings";
import VoiceSettings from "./VoiceSettings";
import EmbeddingSettings from "./EmbeddingSettings";
import McpSettings from "./McpSettings";
export default function SettingsPanel({
  onClose,
  onRestore,
  onChanged,
}: {
  onClose: () => void;
  onRestore: (id: string) => void;
  onChanged: () => void;
}) {
  const [settings, setSettings] = useState<Settings>({
    configured: false,
    base_url: "",
    model: "",
    has_key: false,
    disable_reasoning: false,
    max_output_tokens: null,
    output_token_parameter: "max_tokens",
  });
  const [key, setKey] = useState(""),
    [clearKey, setClearKey] = useState(false),
    [error, setError] = useState(""),
    [notice, setNotice] = useState(""),
    [busy, setBusy] = useState(false),
    [mcpBusy, setMcpBusy] = useState(false),
    [backupBusy, setBackupBusy] = useState(false),
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
      maxOutputTokens: settings.max_output_tokens,
      outputTokenParameter: settings.output_token_parameter,
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
        if (!busy && !mcpBusy && !backupBusy) onClose();
      }}
    >
      <DesktopSettings />
      <VoiceSettings />
      <EmbeddingSettings />
      <BackupSettings disabled={busy || mcpBusy} onBusyChange={setBackupBusy} onRestore={onRestore} />
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
            disabled={busy || backupBusy}
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
            disabled={busy || backupBusy || !ready}
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
            disabled={busy || backupBusy || !ready}
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
              disabled={busy || backupBusy}
              onChange={(e) => setClearKey(e.target.checked)}
            />
            清除已保存的 Key
          </label>
        )}
        <label className="checkbox-label">
          <input
            type="checkbox"
            checked={settings.disable_reasoning}
            disabled={busy || backupBusy || !ready}
            onChange={(e) =>
              setSettings((s) => ({
                ...s,
                disable_reasoning: e.target.checked,
              }))
            }
          />
          快速响应（关闭模型推理，端点需支持）
        </label>
        <details className="model-output-settings">
          <summary>高级：模型输出上限</summary>
          <p className="field-help">默认使用服务商的输出上限。若长文整理被截断，按模型支持的范围填写。部分推理模型的额度包含推理用量。</p>
          <label>输出 token 上限<input type="number" min={1} max={1048576} step={1} placeholder="使用服务商默认值" aria-label="模型输出 token 上限" value={settings.max_output_tokens ?? ""} disabled={busy || backupBusy || !ready}
            onChange={e => setSettings(s => ({ ...s, max_output_tokens: e.target.value === "" ? null : Number(e.target.value) }))} /></label>
          <label>服务商支持的参数<select aria-label="模型输出上限参数" value={settings.output_token_parameter} disabled={busy || backupBusy || !ready} onChange={e => setSettings(s => ({ ...s, output_token_parameter: e.target.value as Settings["output_token_parameter"] }))}>
            <option value="max_tokens">max_tokens（兼容端点）</option><option value="max_completion_tokens">max_completion_tokens（含推理额度）</option>
          </select></label>
        </details>
        <p className="field-help">
          Key 单独保存在本机配置文件中。更换 API 地址时需要重新填写
          Key。连接测试只发送合成测试内容，不读取记忆。
        </p>
        <div className="action-row">
          <button
            className="send-button"
            disabled={
              busy || backupBusy ||
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
              busy || backupBusy ||
              !ready ||
              !settings.base_url.trim() ||
              !settings.model.trim()
            }
            onClick={() =>
              void run(async () => {
                await save();
                const capabilities=await call<NonNullable<Settings["model_capabilities"]>>("workspace_test_model");
                const current=await call<Settings>("workspace_settings");
                setSettings(current);
                if(!current.model_capabilities) return "配置已变化，请对当前配置重新测试连接。";
                if(!capabilities?.single_tool) return "连接测试完成：不支持工具调用，自动整理已暂停；已保存记忆仍可使用。";
                if(!capabilities.multi_turn) return "连接测试完成：支持单次工具，问答和整理使用基本流程。";
                return capabilities.structured_json?"连接测试通过：支持多轮检索与阅读，已启用增强问答和整理。":"支持多轮检索与阅读；结构化 JSON 未通过，正文整理等固定生成能力可能不可用。";
              })
            }
          >
            {busy ? "处理中…" : "保存并测试连接"}
          </button>
        </div>
        {settings.configured&&<p className="field-help">{settings.model_capabilities?.multi_turn?"问答和录入整理可按需补查、补读当前记忆。":settings.model_capabilities?.single_tool===false?"当前模型不支持工具调用，自动整理暂停。":settings.model_capabilities?"当前使用基本问答和单次整理流程。":"尚未验证工具能力，暂用基本流程；测试连接后可启用多轮检索。"}</p>}
      </section>
      <McpSettings onBusyChange={setMcpBusy} />
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
              disabled={busy || backupBusy}
              onClick={() => setRebuild(false)}
            >
              取消
            </button>
            <button
              className="send-button"
              disabled={busy || backupBusy}
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
