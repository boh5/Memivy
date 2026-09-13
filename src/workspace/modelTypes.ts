import i18n from "../i18n";

export type Kind = "llm" | "embedding" | "voice";
export type Binding = {source:"local"|"service";connection:string;model:string;dimensions:number|null;disable_reasoning:boolean;max_output_tokens:number|null;output_token_parameter:"max_tokens"|"max_completion_tokens"};
export type Connection = {id:string;name:string;base_url:string;has_key:boolean};
export type Models = {revision:string;connections:Connection[];llm:Binding|null;embedding:Binding;voice:Binding;auto_organize:boolean};
export type EmbeddingStatus = {enabled:boolean;preparing:boolean;paused:boolean;state:string;downloaded:number;bytes:number;processed:number;total:number;failed:number;error:string|null};
export const emptyBinding = ():Binding => ({source:"local",connection:"",model:"",dimensions:null,disable_reasoning:false,max_output_tokens:null,output_token_parameter:"max_tokens"});
// Keep these module-level values as translation keys. Resolving them here would
// freeze the first language for consumers that import this module once.
type ModelNameKey = "model.names.llm" | "model.names.embedding" | "model.names.voice";
type IndexLabelKey =
  | "embedding.status.reading"
  | "embedding.status.failed"
  | "embedding.status.paused"
  | "embedding.status.downloading"
  | "embedding.status.warming"
  | "embedding.status.indexing"
  | "embedding.indexed"
  | "embedding.status.disabled";
export const modelNames = {
  llm: "model.names.llm",
  embedding: "model.names.embedding",
  voice: "model.names.voice",
} as const satisfies Record<Kind, ModelNameKey>;
const translate=(key:IndexLabelKey,values?:Record<string,unknown>)=>String(i18n.t(key,{ns:"settings",...values}));
export const indexLabel=(s:EmbeddingStatus|null)=>!s?translate("embedding.status.reading"):s.error?translate("embedding.status.failed"):s.paused?translate("embedding.status.paused"):s.state==="downloading"?translate("embedding.status.downloading"):s.state==="warming"?translate("embedding.status.warming"):s.preparing?translate("embedding.status.indexing"):s.enabled?(s.failed>0?translate("embedding.status.failed"):translate("embedding.indexed",{processed:s.processed,total:s.total})):translate("embedding.status.disabled");

export type ConnectionDraft = {id:string;name:string;base_url:string;api_key:string|null;remove:false};
