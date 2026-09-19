import {invoke as nativeInvoke} from '@tauri-apps/api/core';
const pending = new Set<Promise<unknown>>();
let frozen = false;
const allowed = new Set(['draft_read','draft_write','draft_clear','desktop_exit_ready','desktop_state','desktop_modal','update_status']);
export function resumeUpdate() { frozen = false; document.body.inert = false; }
export async function freezeForUpdate() {
  frozen = true;
  document.body.inert = true;
  await Promise.allSettled([...pending]);
}
export function invoke<T>(name:string,args?:Record<string,unknown>):Promise<T> {
  if (frozen && !allowed.has(name)) return Promise.reject({code:'update_busy'});
  const promise = nativeInvoke<T>(name,args);
  if (name !== 'update_install') {
    pending.add(promise);
    void promise.finally(()=>pending.delete(promise)).catch(()=>{});
  }
  return promise;
}
