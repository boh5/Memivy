export type Kind = "llm" | "embedding" | "voice";
export type Binding = {source:"local"|"service";connection:string;model:string;dimensions:number|null;query_prefix:string;disable_reasoning:boolean;max_output_tokens:number|null;output_token_parameter:"max_tokens"|"max_completion_tokens"};
export type Connection = {id:string;name:string;base_url:string;has_key:boolean};
export type Models = {revision:string;connections:Connection[];llm:Binding|null;embedding:Binding;voice:Binding;auto_organize:boolean};
export type EmbeddingStatus = {enabled:boolean;preparing:boolean;paused:boolean;state:string;downloaded:number;bytes:number;processed:number;total:number;failed:number;error:string|null};
export const emptyBinding = ():Binding => ({source:"local",connection:"",model:"",dimensions:null,query_prefix:"",disable_reasoning:false,max_output_tokens:null,output_token_parameter:"max_tokens"});
export const modelNames:Record<Kind,string>={llm:"问答与整理",embedding:"语义检索",voice:"语音输入"};
export const modelHints:Record<Kind,string>={llm:"回答问题，接着讨论，整理留下的想法。",embedding:"换一种说法，也能找回相关记忆。",voice:"开口记下想法，也可以直接提问。"};
export const indexLabel=(s:EmbeddingStatus|null)=>!s?"读取状态…":s.error?"准备未完成":s.paused?"准备已暂停":s.state==="downloading"?"正在下载模型":s.state==="warming"?"正在加载模型":s.preparing?"正在建立索引":s.enabled?`已索引 ${s.processed} / ${s.total} 条`:"未开启";

export type ConnectionDraft = {id:string;name:string;base_url:string;api_key:string|null;remove:false};
