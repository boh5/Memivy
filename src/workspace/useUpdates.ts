import {useEffect, useState} from 'react';
import {listen} from '@tauri-apps/api/event';
import {call, errorText, native} from './api';
import {useNotice} from '../i18n/react';

type UpdateStatus = {revision:number;phase:string;currentVersion:string;version:string|null;notes:string|null;downloaded:number;total:number|null;error:string|null;automatic:boolean};

export function useUpdates() {
  const [status,setStatus]=useState<UpdateStatus|null>(null);
  const [error,setError]=useNotice();
  const [attempt,setAttempt]=useState(0);
  useEffect(()=>{
    if(!native)return;
    let live=true;
    const accept=(next:UpdateStatus)=>{if(live)setStatus(current=>!current||next.revision>current.revision?next:current)};
    const stop=listen<UpdateStatus>('update-status',e=>accept(e.payload));
    void stop.then(()=>{
      if(!live)return;
      return call<UpdateStatus>('update_status').then(accept);
    }).catch(e=>{if(live)setError(errorText(e))});
    return ()=>{live=false;void stop.then(f=>f()).catch(()=>{})};
  },[attempt]);
  return {status,error,setError,retry:()=>{setError('');setAttempt(value=>value+1)}};
}

export function installUpdate() {
  return call('update_install').catch(error=>window.dispatchEvent(new CustomEvent('update-install-error',{detail:error})));
}
