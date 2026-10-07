import {useRef, useState} from 'react';
import {useTranslation} from 'react-i18next';
import {useUpdates, installUpdate} from './useUpdates';

export default function UpdateNotice() {
  const {t}=useTranslation('settings');
  const {status}=useUpdates();
  const [dismissed,setDismissed]=useState<string|null>(null);
  const [requesting,setRequesting]=useState(false);
  const acting=useRef(false);
  if(status?.phase!=='ready'||!status.version||dismissed===status.version)return null;
  async function install() {
    if(acting.current)return;
    acting.current=true;setRequesting(true);
    try {await installUpdate()}
    finally {acting.current=false;setRequesting(false)}
  }
  return <aside className="update-notice" aria-label={t('updates.title')}>
    <span role="status">{t('updates.ready',{version:status.version})}</span>
    <button className="outline-button" disabled={requesting} onClick={()=>void install()}>{t('updates.install')}</button>
    <button className="quiet-button" disabled={requesting} onClick={()=>setDismissed(status.version)}>{t('updates.later')}</button>
  </aside>;
}
