import { useTranslation } from "react-i18next";
import { Icon } from "../ui";
import { indexLabel, modelNames, type EmbeddingStatus, type Kind, type Models } from "./modelTypes";
import type { VoiceStatus } from "./useVoice";

/** Presentation only. Settings owns model reads, drafts and navigation. */
export default function ModelOverview({ models, embedding, voice, disabled, onConfigure }: {
  models: Models | null;
  embedding: EmbeddingStatus | null;
  voice: VoiceStatus | null;
  disabled: boolean;
  onConfigure: (kind: Kind) => void;
}) {
  const { t } = useTranslation("settings");
  return <div className="model-capabilities">
    {(Object.keys(modelNames) as Kind[]).map(kind => {
      const binding = models?.[kind];
      const enabled = kind === "llm" ? !!binding : kind === "embedding"
        ? !!(embedding?.enabled || embedding?.preparing || embedding?.paused) : !!voice?.enabled;
      const status = !models ? t("status.readingSettings") : kind === "embedding" ? indexLabel(embedding)
        : kind === "voice" ? !voice ? t("status.readingSettings")
          : voice.session?.error ? t("voice.status.transcriptionFailed")
          : voice.error ? t("voice.status.needsCheck") : t(enabled ? "status.enabled" : "status.disabled")
        : t(enabled ? "status.configured" : "status.notConfigured");
      const model = binding?.source === "local"
        ? kind === "embedding" ? "Qwen3-Embedding · 0.6B" : "Qwen3-ASR · 0.6B"
        : binding?.model;
      return <section key={kind} className="model-capability" aria-labelledby={`model-${kind}`}>
        <div className="model-cap-icon"><Icon name={kind === "llm" ? "spark" : kind === "embedding" ? "search" : "mic"} size={21} /></div>
        <div className="model-cap-body">
          <h3 id={`model-${kind}`}>{t(modelNames[kind])}</h3>
          <div className="model-cap-meta">
            {model && <span className="model-cap-name">{model}</span>}
            <span className="model-cap-status">{status}</span>
          </div>
        </div>
        <button className="model-text-button" disabled={!models || disabled} onClick={() => onConfigure(kind)}
          aria-label={`${t(enabled ? "actions.manage" : "actions.configure")} ${t(modelNames[kind])}`}>
          {t(enabled ? "actions.manage" : "actions.configure")}<Icon name="chevron" size={14} />
        </button>
      </section>;
    })}
  </div>;
}
