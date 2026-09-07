import { invoke, isTauri } from "@tauri-apps/api/core";
export const native = isTauri();
export type Source = { kind: "capture" | "version"; id: string };
export type SourceEvidence = {
  source: Source;
  title: string;
  text: string;
  truncated: boolean;
  recorded_at: number;
  current: boolean;
  start: number;
};
export type Key = { kind: "memory" | "capture"; id: string };
export type Origin = {
  kind: "user" | "agent" | "conversation";
  app?: string;
  project?: string | null;
  uri?: string | null;
  conversation_id?: string;
};
export type Raw = {
  id: string;
  text: string;
  origin: Origin;
  created_at: number;
  understanding: string;
};
export type Row = {
  key: Key;
  title: string;
  snippet: string;
  updated_at: number;
  origin: Origin | null;
  matched_capture: string | null;
};
export type Page = { items: Row[]; next_offset: number | null };
export type Query = {
  query: string;
  trash: boolean;
  origin?: string;
  project?: string;
  since?: number;
  until?: number;
  offset?: number;
  limit?: number;
};
export type Version = {
  id: string;
  memory_id: string;
  parent_id: string | null;
  title: string;
  body: string;
  actor: "user" | "ai";
  reason: string;
  created_at: number;
  capture_ids: string[];
};
export type Detail = {
  reviewed_conclusion?: { title: string; body: string } | null;
  key: Key;
  state: string;
  title: string;
  body: string;
  current: Version | null;
  history: Version[];
  sources: { id: string; capture: Raw | null }[];
};
export type Draft = {
  key: string;
  request_id: string;
  title: string;
  body: string;
  expected_version: string | null;
  context?: Source[];
  origin?: Origin;
};
export type Receipt = {
  status: "applied" | "needs_review" | "undone";
  request_id: string;
  capture_id: string | null;
  before_version: string | null;
  memory_id: string | null;
  after_version: string | null;
  action: string;
};
export type Topic = { id: string; title: string; updated_at: number };
export type Message = {
  answer?: { recollections: { text: string; sources: Source[] }[]; ideas: string; conclusion: string } | null;
  seq: number;
  id: string;
  text: string;
  role: string;
  turn_id: string;
  status: string;
  error_code: string | null;
  citations: { source: Source; available: boolean }[];
};
export type Settings = {
  configured: boolean;
  base_url: string;
  model: string;
  has_key: boolean;
  disable_reasoning: boolean;
};
export const keyOf = (key: Key) => `${key.kind}:${key.id}`;
export const uid = () => crypto.randomUUID();
export const date = (n: number) =>
  new Date(n).toLocaleDateString("zh-CN", { month: "long", day: "numeric" });
export const fullDate = (n: number) =>
  new Date(n).toLocaleString("zh-CN", { hour12: false });
export const sourceName = (o: Origin | null) =>
  o?.kind === "conversation" ? "确认的讨论结论" : o?.app || "原始记录";
export const errorText = (error: unknown) =>
  typeof error === "string" ? error : "操作未完成，内容仍保留，请重试。";
const previewId = "00000000-0000-4000-8000-000000000001";
const previewRaw: Raw = {
  id: previewId,
  text: "我决定先把 macOS 上的记录、查找和阅读做好。\n\n想法不用整理完整，也应该能放心留下。",
  origin: { kind: "user", app: "Memivy", project: "产品想法" },
  created_at: 1788667200000,
  understanding: "pending",
};
const previewRow: Row = {
  key: { kind: "capture", id: previewId },
  title: "先把桌面体验做好",
  snippet: previewRaw.text,
  updated_at: previewRaw.created_at,
  origin: previewRaw.origin,
  matched_capture: null,
};
export async function call<T>(
  name: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (native) return invoke<T>(name, args);
  if (name === "discussion_targets") return [] as T;
  if (name === "organization_jobs") return [] as T;
  if (name === "library_query") {
    const q = args?.query as Query;
    return {
      items:
        q.trash || (q.query && !previewRaw.text.includes(q.query))
          ? []
          : [previewRow],
      next_offset: null,
    } as T;
  }
  if (name === "library_detail")
    return {
      key: previewRow.key,
      state: "active",
      title: previewRow.title,
      body: previewRaw.text,
      current: null,
      history: [],
      sources: [{ id: previewId, capture: previewRaw }],
    } as T;
  if (
    name === "library_topics" ||
    name === "library_messages" ||
    name === "discussion_messages"
  )
    return [] as T;
  if (name === "library_projects") return ["产品想法"] as T;
  if (name === "draft_read") return null as T;
  if (name === "workspace_settings")
    return {
      configured: false,
      base_url: "",
      model: "",
      has_key: false,
      disable_reasoning: false,
    } as T;
  throw "浏览器仅预览布局；请在 Mac 应用里保存和编辑真实记忆。";
}
