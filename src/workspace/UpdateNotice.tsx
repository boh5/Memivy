import { useState } from "react";
import { useTranslation } from "react-i18next";
import { translateCatalog } from "../i18n";
import { renderMessage } from "../i18n/messages";
import { ErrorNotice } from "./components";
import { useUpdates, installUpdate } from "./useUpdates";

export default function UpdateNotice() {
  const { t } = useTranslation("settings");
  const { status, installation, installProgress, installError, readyToInstall } = useUpdates();
  const [dismissed, setDismissed] = useState<{ version: string | null; failedAttempt?: number } | null>(null);
  const version = status?.version ?? null;
  const failure = renderMessage(installError, translateCatalog);
  const hidden = dismissed?.version === version && (!failure || dismissed.failedAttempt === installation.attempt);
  if (!installProgress && (!readyToInstall || hidden)) return null;
  const canInstall = readyToInstall && !installProgress;

  return <aside className="update-notice" aria-label={t("updates.title")}>
    <span role="status">{installProgress ? t(installProgress) : version ? t("updates.ready", { version }) : t("updates.title")}</span>
    <button className="outline-button" aria-disabled={!canInstall} onClick={() => { if (canInstall) void installUpdate(); }}>
      {t(failure ? "updates.retryInstall" : "updates.install")}
    </button>
    {!installProgress &&
      <button className="quiet-button" onClick={() => setDismissed({ version, failedAttempt: failure ? installation.attempt : undefined })}>
        {t("updates.later")}
      </button>
    }
    <ErrorNotice text={failure} />
  </aside>;
}
