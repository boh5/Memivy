import {useEffect, useRef, useState} from 'react';
import {flushSync} from 'react-dom';
import {listen} from '@tauri-apps/api/event';
import {useTranslation} from 'react-i18next';
import {call, errorText, native} from './api';
import {renderMessage} from '../i18n/messages';
import {translateCatalog} from '../i18n';
import {useNotice} from '../i18n/react';
import {ErrorNotice} from './components';
type Status = {phase:string;currentVersion:string;version:string|null;notes:string|null;downloaded:number;total:number|null;error:string|null};
export default function UpdateSettings({onClose}:{onClose:()=>void}) {
  const {t}=useTranslation('settings');
  const [status,setStatus]=useState<Status|null>(null),[error,setError]=useNotice();
  const acting=useRef(false);
  const [requesting,setRequesting]=useState(false);
  useEffect(()=>{
    if(!native)return;
    let live=true, events=0;
    const stop=listen<Status>('update-status',e=>{events++;if(live)setStatus(e.payload)});
    void stop.then(()=>{const observed=events;return call<Status>('update_status').then(s=>{if(live&&events===observed)setStatus(s)})}).catch(e=>{if(live)setError(errorText(e))});
    return ()=>{live=false;void stop.then(f=>f())};
  },[]);
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
  if(!status)return null;
  const phase=status.phase;
  return <section className="settings-section">
    <h3>{t('updates.title')}</h3>
    <p>{t('updates.currentVersion',{version:status.currentVersion})}</p>
    {phase==='disabled'?<p>{t('updates.disabled')}</p>:<>
      {status.version&&<p>{t('updates.available',{version:status.version})}</p>}
      {status.notes&&<p style={{whiteSpace:'pre-wrap',maxHeight:180,overflow:'auto'}}>{status.notes}</p>}
      {phase==='current'&&<p role="status">{t('updates.current')}</p>}
      {phase==='downloading'&&<><p role="status">{t('updates.downloading')}</p><progress aria-label={t('updates.downloading')} max={status.total||undefined} value={status.total?status.downloaded:undefined}/></>}
      {phase==='checking'&&<p role="status">{t('updates.checking')}</p>}
      {phase==='ready'?<button className="send-button" onClick={install}>{t('updates.install')}</button>:
        phase==='available'?<button className="send-button" disabled={requesting} onClick={()=>void act('update_download')}>{t('updates.download')}</button>:
        <button className="outline-button" disabled={requesting||!['idle','current'].includes(phase)} onClick={()=>void act('update_check')}>{t('updates.check')}</button>}
    </>}
    <ErrorNotice text={error||(status.error?renderMessage(errorText({code:status.error}),translateCatalog):'')}/>
  </section>;
}
