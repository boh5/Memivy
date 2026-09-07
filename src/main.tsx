import { createRoot } from "react-dom/client";
import App from "./workspace/App";
import Prototype from "./Prototype";
const prototype = new URLSearchParams(location.search).has("prototype");
document.title = prototype ? "Memivy · 交互样机 v2" : "Memivy";
document.documentElement.dataset.surface =
  prototype && location.search.includes("window=capture")
    ? "companion"
    : "workspace";
createRoot(document.getElementById("root")!).render(
  prototype ? <Prototype /> : <App />,
);
