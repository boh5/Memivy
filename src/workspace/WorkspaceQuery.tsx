import { useCallback, useEffect, useRef, useState, type ComponentProps } from "react";
import WorkspaceTopBar from "./WorkspaceTopBar";
import CaptureForm from "./CaptureForm";

// The draft preview belongs to the query surface, not the whole workspace.
export default function WorkspaceQuery({ bar, form, session }: {
  bar: Omit<ComponentProps<typeof WorkspaceTopBar>, "preview" | "children">;
  form: Omit<ComponentProps<typeof CaptureForm>, "onDraftChange">;
  session: string;
}) {
  const [preview, setPreview] = useState("");
  const latestDraft = useRef("");
  const expanded = useRef(bar.open); expanded.current = bar.open;
  const reportDraft = useCallback((body: string) => {
    latestDraft.current = body;
    // While composing, the expanded input is the only draft display that changes.
    if (!expanded.current) setPreview(body);
  }, []);
  useEffect(() => { if (!bar.open) setPreview(latestDraft.current); }, [bar.open]);
  return <WorkspaceTopBar {...bar} preview={preview}>
    <CaptureForm key={session} {...form} onDraftChange={reportDraft} />
  </WorkspaceTopBar>;
}
