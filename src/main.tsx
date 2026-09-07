import { createRoot } from "react-dom/client";
import App from "./workspace/App";
import Prototype from "./Prototype";
import Desktop from "./workspace/Desktop";
const capture = new URLSearchParams(location.search).get("window") === "capture";
const prototype = new URLSearchParams(location.search).has("prototype");
document.title = prototype ? "Memivy · 交互样机 v2" : "Memivy";
document.documentElement.dataset.surface =
  capture
    ? "companion"
    : "workspace";
createRoot(document.getElementById("root")!).render(
  prototype ? <Prototype /> : capture ? <Desktop /> : <App />,
);
