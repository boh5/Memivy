import {useEffect, useRef, useState} from 'react';
import {flushSync} from 'react-dom';
import {listen} from '@tauri-apps/api/event';
import {useTranslation} from 'react-i18next';
import {call, errorText, native} from './api';
import {renderMessage} from '../i18n/messages';
import {translateCatalog} from '../i18n';
import {useNotice} from '../i18n/react';
import {ErrorNotice} from './components';
import Markdown from './Markdown';
import {Icon} from '../ui';
type Status = {phase:string;currentVersion:string;version:string|null;notes:string|null;downloaded:number;total:number|null;error:string|null};
export default function UpdateSettings({onClose}:{onClose:()=>void}) {
  const {t}=useTranslation('settings');
  const [status,setStatus]=useState<Status|null>(null),[error,setError]=useNotice();
  const acting=useRef(false);
  const [requesting,setRequesting]=useState(false);
  const [attempt,setAttempt]=useState(0);
  useEffect(()=>{
    if(!native)return;
    let live=true, events=0;
    const stop=listen<Status>('update-status',e=>{events++;if(live)setStatus(e.payload)});
    void stop.then(()=>{const observed=events;return call<Status>('update_status').then(s=>{if(live&&events===observed)setStatus(s)})}).catch(e=>{if(live)setError(errorText(e))});
    return ()=>{live=false;void stop.then(f=>f()).catch(()=>{})};
  },[attempt]);
  async function act(command:string) {
    if(acting.current)return;
    acting.current=true;setRequesting(true);setError('');
    try {await call(command)} catch(e){setError(errorText(e))}
    finally {acting.current=false;setRequesting(false)}
  }
  function install() {
    flushSync(onClose);
    void call('update_install').catch(error=>window.dispatchEvent(new CustomEvent('update-install-error',{detail:error})));
  }
  if(!status)return error?<section className="settings-section">
    <h3>{t('updates.title')}</h3>
    <ErrorNotice text={error}/>
    <button className="outline-button" onClick={()=>{setError('');setAttempt(value=>value+1)}}>{t('actions.retry')}</button>
  </section>:null;
  const phase=status.phase;
  // Omit the repeated version heading and build footer added by our release workflow.
  const notes=status.notes?.replace(/^## (\d+\.\d+\.\d+)\r?\n+/, (heading,version)=>version===status.version?'':heading)
    .replace(/\n+Source commit: [a-f0-9]{40}\s*$/i,'').trim();
  return <section className="settings-section update-settings">
    <div className="update-heading">
      <div>
        <h3>{t('updates.title')}</h3>
        <p className="update-version">{t('updates.currentVersion',{version:status.currentVersion})}</p>
      </div>
      {phase!=='disabled'&&(phase==='ready'?<button className="send-button" onClick={install}><Icon name="refresh" size={15}/>{t('updates.install')}</button>:
        phase==='available'?<button className="send-button" disabled={requesting} onClick={()=>void act('update_download')}><Icon name="download" size={15}/>{t('updates.download')}</button>:
        <button className="outline-button" disabled={requesting||!['idle','current'].includes(phase)} onClick={()=>void act('update_check')}>{t('updates.check')}</button>)}
    </div>
    {phase==='disabled'?<p className="update-status">{t('updates.disabled')}</p>:<>
      {phase==='current'&&<p className="update-status update-current" role="status"><Icon name="check" size={15}/>{t('updates.current')}</p>}
      {phase==='downloading'&&<div className="update-download"><p className="update-status" role="status">{t('updates.downloading')}</p><progress aria-label={t('updates.downloading')} max={status.total||undefined} value={status.total?status.downloaded:undefined}/></div>}
      {phase==='checking'&&<p className="update-status" role="status">{t('updates.checking')}</p>}
      {(status.version||notes)&&<div className="update-release">
        <div className="update-release-heading">
          <h4>{t('updates.releaseNotes')}</h4>
          {status.version&&<span className="update-available">{t('updates.available',{version:status.version})}</span>}
        </div>
        {notes&&<div className="update-notes" role="region" aria-label={t('updates.releaseNotes')} tabIndex={0}><Markdown text={notes}/></div>}
      </div>}
    </>}
    <ErrorNotice text={error||(status.error?renderMessage(errorText({code:status.error}),translateCatalog):'')}/>
  </section>;
}
