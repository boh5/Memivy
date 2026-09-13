import { createRoot } from "react-dom/client";
import App from "./workspace/App";
import Desktop from "./workspace/Desktop";
import { startLanguage } from "./i18n/preferences";
const isCompanionWindow = new URLSearchParams(location.search).get("window") === "capture";
document.title = "Memivy";
document.documentElement.dataset.surface = isCompanionWindow ? "companion" : "workspace";
async function mount() {
  await startLanguage();
  createRoot(document.getElementById("root")!).render(
    isCompanionWindow ? <Desktop /> : <App />,
  );
}
void mount();
