import { resourceCall } from "./resources";
import { isTauri } from "@tauri-apps/api/core";
import i18n from '../i18n';
import { formatDate, formatFullDate } from '../i18n/format';
import { message, type UiMessage } from '../i18n/messages';
import errors from '../../locales/en/errors.json';
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
  additional_spans?: {start:number;text:string;truncated:boolean}[];
};
export type Key = { kind: "memory" | "capture"; id: string };
export type Origin = {
  kind: "user" | "agent" | "conversation" | "discussion";
  app?: string;
  project?: string | null;
  uri?: string | null;
  conversation_id?: string;
  message_id?: string;
};
export type CaptureResult = { memory_id: string; version_id: string; capture_id: string; created_at: number };
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
};
export type Page = { items: Row[]; next_offset: number | null; degraded_reason?: string | null };
export type Query = {
  query: string;
  trash: boolean;
  origin?: string;
  project?: string;
  since?: number;
  until?: number;
  offset?: number;
  limit?: number;
  pinned?: boolean;
  collection_id?: string;
  exclude_collection_id?: string;
  oldest?: boolean;
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
  key: Key;
  state: string;
  title: string;
  body: string;
  current: Version | null;
  history: Version[];
  history_count?: number;
  source_count?: number;
  sources: { id: string; capture: Raw | null; conversation_available: boolean | null }[];
};
export type ConclusionDestination = { kind: "new" } | { kind: "existing"; memory_id: string; expected_version: string };
export type Draft = {
  destination?: ConclusionDestination;
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
export type Topic = { id: string; title: string; updated_at: number; collection_id?: string | null };
export type Collection = { id: string; name: string; description: string; revision: number; count: number };
export type RecordNavigation = { pinned: boolean; collections: string[] };
export type Message = {
  followups: string[];
  receipts: Receipt[];
  progress: string | null;
  record_only: boolean;
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
  model_capabilities?: {structured_json:boolean;streaming_text:boolean;single_tool:boolean;multi_turn:boolean}|null;
  configured: boolean;
  base_url: string;
  model: string;
  has_key: boolean;
  disable_reasoning: boolean;
  max_output_tokens: number | null;
  output_token_parameter: "max_tokens" | "max_completion_tokens";
};
export const keyOf = (key: Key) => `${key.kind}:${key.id}`;
export const uid = () => crypto.randomUUID();
export const date = formatDate;
export const fullDate = formatFullDate;
export const sourceName = (o: Origin | null) =>
  o?.kind === "discussion" ? i18n.t('sourceDiscussion') : o?.kind === "conversation" ? i18n.t('sourceConclusion') : o?.app || i18n.t('sourceCapture');
export function errorText(error: unknown): UiMessage {
  const code = error && typeof error === 'object' && 'code' in error ? error.code : undefined;
  return message('errors', typeof code === 'string' && Object.hasOwn(errors, code) ? code as keyof typeof errors : 'operation_failed');
}
export const unavailable = (error: unknown) => !!error && typeof error === "object" && "code" in error && error.code === "unavailable";
const previewId = "00000000-0000-4000-8000-000000000001";
const previewRaw: Raw = {
  id: previewId,
  text: "I decided to focus on capturing, finding, and reading notes on macOS first.\n\nIdeas should be easy to save, even before they are fully formed.",
  origin: { kind: "user", app: "Memivy", project: "Product ideas" },
  created_at: 1788667200000,
  understanding: "pending",
};
const previewRow: Row = {
  key: { kind: "capture", id: previewId },
  title: "Focus on the desktop experience",
  snippet: previewRaw.text,
  updated_at: previewRaw.created_at,
  origin: previewRaw.origin,
};
export async function call<T>(
  name: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (native) return resourceCall<T>(name, args);
  if (name === "models_load") return {revision:"preview",connections:[],llm:null,embedding:{source:"local",connection:"",model:"",dimensions:null,disable_reasoning:false,max_output_tokens:null,output_token_parameter:"max_tokens"},voice:{source:"local",connection:"",model:"",dimensions:null,disable_reasoning:false,max_output_tokens:null,output_token_parameter:"max_tokens"},auto_organize:true} as T;
  if (name === "embedding_status") return {enabled:false,preparing:false,paused:false,state:"not_downloaded",downloaded:0,bytes:639150592,processed:0,total:1,failed:0,error:null} as T;
  if (name === "voice_status") return {source:"local",label:"voice_local",local_available:false,enabled:false,preload:false,shortcut:"Alt+KeyR",state:"unloaded",backend:null,error:null,available:false,downloaded:0,bytes:1019141728,cache:"",session:null} as T;
  if (name === "navigation_collections") return [] as T;
  if (name === "navigation_record") return { pinned: false, collections: [] } as T;
  if (name === "organization_jobs") return [] as T;
  if (name === "library_query") {
    const q = args?.query as Query;
    return {
      items:
        q.pinned || q.collection_id || q.trash || (q.query && !previewRaw.text.includes(q.query))
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
    name === "library_agent_changes" ||
    name === "library_topics" ||
    name === "discussion_messages"
  )
    return [] as T;
  if (name === "library_projects") return ["Product ideas"] as T;
  if (name === "draft_read") return null as T;
  if (name === "workspace_settings")
    return {
      configured: false,
      base_url: "",
      model: "",
      has_key: false,
      disable_reasoning: false,
    } as T;
  throw { code: 'native_required' };
}
