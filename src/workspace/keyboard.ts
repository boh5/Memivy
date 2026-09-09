type SubmitKey = {
  key: string; metaKey: boolean; ctrlKey: boolean; altKey: boolean;
  shiftKey: boolean; repeat: boolean; isComposing?: boolean; keyCode?: number;
};
export function isSubmitKey(e: SubmitKey, composing = false) {
  return e.key === "Enter" && e.metaKey && !e.ctrlKey && !e.altKey &&
    !e.shiftKey && !e.repeat && !e.isComposing && !composing && e.keyCode !== 229;
}

// A search field submits with Return; Shift+Return still permits a longer query.
export function isRecallSubmitKey(e: SubmitKey, composing = false) {
  return e.key === "Enter" && !e.ctrlKey && !e.altKey && !e.shiftKey &&
    !e.repeat && !e.isComposing && !composing && e.keyCode !== 229;
}
