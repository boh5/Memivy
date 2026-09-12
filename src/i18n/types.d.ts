import 'i18next';
import common from '../../locales/en/common.json';
import workspace from '../../locales/en/workspace.json';
import settings from '../../locales/en/settings.json';
import editor from '../../locales/en/editor.json';
import errors from '../../locales/en/errors.json';
import native from '../../locales/en/native.json';

declare module 'i18next' {
  interface CustomTypeOptions {
    defaultNS: 'common';
    returnNull: false;
    resources: {
      common: typeof common;
      workspace: typeof workspace;
      settings: typeof settings;
      editor: typeof editor;
      errors: typeof errors;
      native: typeof native;
    };
  }
}
