import UpdateSettings from './UpdateSettings';
import {useRef, useState} from 'react';
import {useTranslation} from 'react-i18next';
import {setLanguagePreference} from '../i18n/preferences';
import {useNotice} from '../i18n/react';
import {call, errorText, native} from './api';
import {ErrorNotice} from './components';
import DesktopSettings from './DesktopSettings';
import LanguageSettings from './LanguageSettings';
import VoiceSettings from './VoiceSettings';
import {restoreGeneralSettings} from './restoreGeneralSettings';

export default function GeneralSettings({focusEntry=false,onBusyChange,onClose}:{focusEntry?:boolean;onBusyChange:(busy:boolean)=>void;onClose:()=>void}) {
  const {t}=useTranslation('settings');
  const [busy,setBusy]=useState(false), [revision,setRevision]=useState(0), [error,setError]=useNotice();
  const acting=useRef(false);
  function changeBusy(value:boolean) { acting.current=value; setBusy(value); onBusyChange(value); }
  async function restore() {
    if(acting.current)return;
    changeBusy(true); setError('');
    try { await restoreGeneralSettings(call,setLanguagePreference); }
    catch(e) { setError(errorText(e)); }
    finally { setRevision(value=>value+1); changeBusy(false); }
  }
  return <div className="settings-page general-settings-page">
    <fieldset className="general-settings-controls" disabled={busy} key={revision}>
      <LanguageSettings onBusyChange={changeBusy}/>
      <DesktopSettings focusEntry={focusEntry} onBusyChange={changeBusy}><VoiceSettings shortcutOnly onBusyChange={changeBusy}/></DesktopSettings>
    </fieldset>
    <UpdateSettings onClose={onClose}/>
    <section className="settings-section general-settings-reset">
      <button className="outline-button" disabled={busy||!native} onClick={()=>void restore()}>{t('general.restoreDefaults')}</button>
      <ErrorNotice text={error}/>
    </section>
  </div>;
}
