import { useState, type Dispatch, type SetStateAction } from 'react';
import { useTranslation } from 'react-i18next';
import { renderMessage, type UiMessage } from './messages';
import { translateCatalog } from './index';

/** Language changes render notices again without restarting their business operation. */
export function useNotice(initial: UiMessage = ''): [string, Dispatch<SetStateAction<UiMessage>>, UiMessage] {
  const [value, setValue] = useState<UiMessage>(initial);
  useTranslation();
  return [renderMessage(value, translateCatalog), setValue, value];
}
