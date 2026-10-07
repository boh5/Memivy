import {useRef, useState} from 'react';
import {flushSync} from 'react-dom';
import {useTranslation} from 'react-i18next';
import {call, errorText} from './api';
import {renderMessage} from '../i18n/messages';
import {translateCatalog} from '../i18n';
import {useUpdates, installUpdate} from './useUpdates';
import {ErrorNotice} from './components';
import Markdown from './Markdown';
import {Icon} from '../ui';
export default function UpdateSettings({onClose,disabled=false,onBusyChange}:{onClose:()=>void;disabled?:boolean;onBusyChange?:(busy:boolean)=>void}) {
  const {t}=useTranslation('settings');
  const {status,error,setError,retry}=useUpdates();
  const acting=useRef(false);
  const [requesting,setRequesting]=useState(false);
  async function act(command:string,args?:Record<string,unknown>) {
    if(acting.current||disabled)return;
    const saving=command==='update_set_automatic';
    acting.current=true;setRequesting(true);if(saving)onBusyChange?.(true);setError('');
    try {await call(command,args)} catch(e){setError(errorText(e))}
    finally {acting.current=false;setRequesting(false);if(saving)onBusyChange?.(false)}
  }
  function install() {
    if(acting.current||disabled)return;
    flushSync(onClose);
    void installUpdate();
  }
  if(!status)return error?<section className="settings-section">
    <h3>{t('updates.title')}</h3>
    <ErrorNotice text={error}/>
    <button className="outline-button" onClick={retry}>{t('actions.retry')}</button>
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
      {phase!=='disabled'&&(phase==='ready'?<button className="send-button" disabled={disabled||requesting} onClick={install}><Icon name="refresh" size={15}/>{t('updates.install')}</button>:
        phase==='available'?<button className="send-button" disabled={disabled||requesting} onClick={()=>void act('update_download')}><Icon name="download" size={15}/>{t('updates.download')}</button>:
        <button className="outline-button" disabled={disabled||requesting||!['idle','current'].includes(phase)} onClick={()=>void act('update_check')}>{t('updates.check')}</button>)}
    </div>
    {phase==='disabled'?<p className="update-status">{t('updates.disabled')}</p>:<>
      <div className="setting-line update-automatic">
        <div><strong>{t('updates.automatic')}</strong><p>{t('updates.automaticDescription')}</p></div>
        <input type="checkbox" role="switch" className="settings-switch" aria-label={t('updates.automatic')} checked={status.automatic} disabled={disabled||requesting||['preparing','installing'].includes(phase)} onChange={e=>void act('update_set_automatic',{automatic:e.target.checked})}/>
      </div>
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
