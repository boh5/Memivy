import type { ImportResult, Source } from "./api";

export type ImportOrganization = { id: string; topicId: string; title: string; text: string; context: Source[]; memories: { memory_id: string; version_id: string }[] };

export function importedSources(result: ImportResult): Source[] {
  return result.items.filter(item => item.status === "imported").map(item => ({ kind: "version", id: item.version_id }));
}

export function importOrganizationInstruction(language: string, memoryIds: readonly string[] = []): string {
  const responseLanguage = language.toLowerCase().startsWith("zh") ? "Simplified Chinese" : "English";
  return `Please organize this batch of imported reference materials into useful memories. Read the selected memories and their original sources before making changes. Imported files may include my own notes, quotations, third-party writing, or tentative ideas; do not assume I wrote or endorsed every statement. Preserve authorship, dates, uncertainty, original evidence, and reversible change receipts. Preserve each target memory's existing language and the original language of quoted source text unless I explicitly ask for translation. The response language instruction applies only to your discussion reply. Do not turn questions or AI suggestions into facts about me. Use the existing memory tools to improve these memories, splitting distinct ideas or merging genuine duplicates only when the evidence supports it. Read relevant existing memories before updating or merging them. Do not add collection membership without my explicit selection. If the batch exceeds this turn's reading or processing limits, report which material remains and ask me to continue; do not claim the whole batch is organized. The imported batch contains these exact memory IDs: ${memoryIds.join(", ")}. If any named imported target is unavailable, stop and report it rather than substituting another memory or broadening the batch. Do not start organizing unrelated memories. Reply in ${responseLanguage}.`;
}
