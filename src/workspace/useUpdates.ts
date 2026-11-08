import {useEffect, useState} from 'react';
import {listen} from '@tauri-apps/api/event';
import {call, errorText, native} from './api';
import {useNotice} from '../i18n/react';
import type {UiMessage} from '../i18n/messages';

type UpdateStatus = {revision:number;phase:string;currentVersion:string;version:string|null;notes:string|null;downloaded:number;total:number|null;error:string|null;automatic:boolean};
type InstallRequest = {attempt:number;pending:boolean;error:UiMessage};
type InstallProgress = 'updates.installing' | 'updates.closingMcp' | 'updates.preparing' | 'updates.requestingInstall';
let installRequest: InstallRequest = {attempt:0,pending:false,error:''};
const installListeners = new Set<(request:InstallRequest)=>void>();

export function useUpdates() {
  const [status,setStatus]=useState<UpdateStatus|null>(null);
  const [error,setError]=useNotice();
  const [attempt,setAttempt]=useState(0);
  const [installation,setInstallation]=useState(()=>installRequest);
  useEffect(()=>{
    installListeners.add(setInstallation);
    setInstallation(installRequest);
    return ()=>{installListeners.delete(setInstallation)};
  },[]);
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
  const installProgress:InstallProgress|null=status?.phase==='installing'?'updates.installing'
    : status?.phase==='closing_mcp'?'updates.closingMcp'
    : status?.phase==='preparing'?'updates.preparing'
    : installation.pending?'updates.requestingInstall':null;
  const readyToInstall=status?.phase==='ready'&&!!status.version;
  const nativeInstallError=status?.error&&!status.error.startsWith('update_preferences_')
    ? errorText({code:status.error}) : '';
  const installError=readyToInstall&&!installProgress ? installation.error||nativeInstallError : '';
  useEffect(()=>{
    // A check or a new download supersedes any local installation IPC error.
    if(status&&!readyToInstall&&!installProgress&&installRequest.error) {
      publishInstallRequest({...installRequest,error:''});
    }
  },[status,readyToInstall,installProgress]);
  return {status,error,setError,installation,installProgress,installError,readyToInstall,retry:()=>{setError('');setAttempt(value=>value+1)}};
}

function publishInstallRequest(next:InstallRequest) {
  installRequest=next;
  installListeners.forEach(listener=>listener(next));
}

// Settings closes before this IPC so both windows can confirm their drafts.
// Keep the request status available to the main notice after Settings unmounts.
export async function installUpdate() {
  if(installRequest.pending)return;
  const attempt=installRequest.attempt+1;
  publishInstallRequest({attempt,pending:true,error:''});
  let error:UiMessage='';
  try {await call('update_install')}
  catch(e) {error=errorText(e)}
  finally {publishInstallRequest({attempt,pending:false,error})}
}
