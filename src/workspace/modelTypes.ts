import i18n from "../i18n";

export type Kind = "llm" | "embedding" | "voice";
export type Provider = "openai_compatible" | "openai_responses" | "anthropic" | "gemini";
export const providerKeys = {openai_compatible:"capability.providers.openaiCompatible",openai_responses:"capability.providers.openaiResponses",anthropic:"capability.providers.anthropic",gemini:"capability.providers.gemini"} as const;
export const providerUrls:Record<Provider,string> = {openai_compatible:"https://api.openai.com/v1",openai_responses:"https://api.openai.com/v1",anthropic:"https://api.anthropic.com",gemini:"https://generativelanguage.googleapis.com"};
export type Binding = {provider:Provider;source:"local"|"service";base_url:string;has_key:boolean;api_key?:string|null;model:string;dimensions:number|null;disable_reasoning:boolean;max_output_tokens:number|null;output_token_parameter:"max_tokens"|"max_completion_tokens"};
export type Models = {revision:string;llm:Binding|null;embedding:Binding;voice:Binding;auto_organize:boolean};
export type EmbeddingStatus = {enabled:boolean;preparing:boolean;paused:boolean;state:string;downloaded:number;bytes:number;processed:number;total:number;failed:number;error:string|null};
export const emptyBinding = ():Binding => ({provider:"openai_compatible",source:"local",base_url:"",has_key:false,model:"",dimensions:null,disable_reasoning:false,max_output_tokens:null,output_token_parameter:"max_tokens"});
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
