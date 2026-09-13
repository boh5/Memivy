import type {LanguagePreference} from '../i18n/languages';
import type {DesktopState} from './desktopApi';
import type {VoiceStatus} from './useVoice';

// Only preferences exposed on the General page belong to this operation.
export async function restoreGeneralSettings(
  call: <T>(name:string,args?:Record<string,unknown>)=>Promise<T>,
  setLanguage: (preference:LanguagePreference)=>Promise<void>,
) {
  const desktop = await call<DesktopState>('desktop_state');
  const voice = await call<VoiceStatus>('voice_status');
  // Free the voice binding first so swapped shortcut assignments can reset.
  await call('voice_control',{action:'shortcut',value:''});
  try {
    await call('desktop_update',{patch:{shortcut:'Alt+KeyM',visible:true}});
    await call('voice_control',{action:'shortcut',value:'Alt+KeyR'});
  } catch(error) {
    try {
      await call('voice_control',{action:'shortcut',value:''});
      await call('desktop_update',{patch:{shortcut:desktop.shortcut,visible:desktop.visible}});
      await call('voice_control',{action:'shortcut',value:voice.shortcut});
    } catch { throw {code:'general_reset_partial'}; }
    throw error;
  }
  try {
    await call('desktop_login',{enabled:true});
    await setLanguage('system');
  } catch { throw {code:'general_reset_partial'}; }
}
