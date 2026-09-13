import {useState} from 'react';
import {useTranslation} from 'react-i18next';
import {shortcutLabel} from './desktopApi';

/** Both bindings use the same recorder; their owners retain persistence and errors. */
export default function ShortcutSetting({label,value,disabled=false,onChange}:{
  label:string; value:string; disabled?:boolean; onChange:(value:string)=>void;
}) {
  const {t}=useTranslation('settings');
  const [recording,setRecording]=useState(false);
  return <div className="setting-line shortcut-setting">
    <strong>{label}</strong>
    <div className="shortcut-controls">
      <button type="button" className={`outline-button shortcut-recorder${recording?' recording':''}`}
        aria-label={`${label} ${recording?t('desktop.pressShortcut'):value?shortcutLabel(value):t('desktop.setShortcut')}`}
        disabled={disabled} onClick={event=>{event.currentTarget.focus();setRecording(true)}}
        onBlur={()=>setRecording(false)} onKeyDown={event=>{
          if(!recording||disabled)return;
          if(event.key==='Tab'){setRecording(false);return;}
          event.preventDefault();event.stopPropagation();
          if(event.key==='Escape'){setRecording(false);return;}
          if(event.repeat||event.nativeEvent.isComposing||['Meta','Control','Alt','Shift'].includes(event.key))return;
          const value=[...(event.ctrlKey?['Control']:[]),...(event.altKey?['Alt']:[]),...(event.shiftKey?['Shift']:[]),...(event.metaKey?['Super']:[]),event.code].join('+');
          setRecording(false);onChange(value);
        }}>{recording?t('desktop.pressShortcut'):value?shortcutLabel(value):t('desktop.setShortcut')}</button>
      <button type="button" className="model-text-button shortcut-remove" aria-label={`${t('desktop.removeShortcut')} ${label}`}
        disabled={disabled||recording||!value} onClick={()=>onChange('')}>{t('actions.remove')}</button>
    </div>
  </div>;
}
