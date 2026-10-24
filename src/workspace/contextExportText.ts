import type { ContextExportSnapshot } from "./api";

export const CONTEXT_EXPORT_LIMIT = 20;
const heading = (value: string) => value.replace(/[\r\n]+/g, " ").replace(/[\\`*_{}\[\]<>#]/g, "\\$&");

export function fullContextText(snapshot: ContextExportSnapshot): string {
  return snapshot.items.map((item, index) => `## [S${index + 1}] ${heading(item.title)}\n\n${item.body}`).join("\n\n---\n\n");
}

export function contextSourceFooter(snapshot: ContextExportSnapshot, labels: {
  sources: string; version: string; captured: string; localSource: string;
  user: string; agent: string; conversation: string; discussion: string; imported: string;
  userMessage: string; assistantMessage: string; confirmedByUser: string;
  application: string; file: string; importedAt: string; source: string; attributionHelp: string;
}, formatDate: (timestamp: number) => string): string {
  return `## ${labels.sources}\n\n${labels.attributionHelp}\n\n` + snapshot.items.map((item, index) => {
    const origins = item.sources.map((source, sourceIndex) => {
      const uri = source.origin.uri?.trim();
      // File paths and custom application URLs are not useful or safe to share.
      const web = uri && /^https?:\/\//i.test(uri) && !/[\s<>]/.test(uri) ? `<${uri}>` : labels.localSource;
      const origin = source.origin;
      const role = origin.kind === "conversation" && origin.message_role
        ? (origin.message_role === "user" ? labels.userMessage : labels.assistantMessage) : "";
      const confirmation = origin.kind === "conversation" && origin.confirmed_by === "user" ? labels.confirmedByUser : "";
      const app = origin.app && !/[\/\\]/.test(origin.app) && !origin.app.trimStart().startsWith("~")
        ? `${labels.application}: ${heading(origin.app)}` : "";
      const file = origin.kind === "imported" && origin.file_name ? `${labels.file}: ${heading(origin.file_name)}` : "";
      const attribution = [labels[origin.kind], role, confirmation, app, file].filter(Boolean).join(" · ");
      const recorded = origin.kind === "imported" ? labels.importedAt : labels.captured;
      return `${labels.source} ${sourceIndex + 1}: ${attribution}\n\n${web} · ${recorded}: ${formatDate(source.recorded_at)}`;
    });
    return `[S${index + 1}] ${heading(item.title)}\n\n${labels.version}: ${item.version_id} · ${formatDate(item.created_at)}${origins.length ? `\n\n${origins.join("\n\n")}` : ""}`;
  }).join("\n\n");
}

export function contextCopyText(body: string, footer: string, scope: string): string {
  return `${scope}\n\n${body.trim()}\n\n---\n\n${footer}\n`;
}
