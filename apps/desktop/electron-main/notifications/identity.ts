import { execFile } from 'node:child_process';

/** The identity an installation's notifications are sent under; setup removes the same. */
export const notificationIdentity = (namespace: string): string => `Aura.${namespace}`;

/**
 * Names the identity for Windows (HKCU\Software\Classes\AppUserModelId\<identity>), so a
 * notification is headed "Aura" rather than with the identity itself. Uninstalling removes it.
 */
export function registerNotificationIdentity(identity: string): Promise<void> {
  if (!/^Aura\.[a-z0-9][a-z0-9.-]*$/u.test(identity))
    return Promise.reject(new Error('notification identity is not valid'));
  return new Promise((done, fail) => {
    execFile(
      'reg.exe',
      [
        'add',
        `HKCU\\Software\\Classes\\AppUserModelId\\${identity}`,
        '/v',
        'DisplayName',
        '/t',
        'REG_SZ',
        '/d',
        'Aura',
        '/f',
      ],
      { windowsHide: true },
      (error) => (error ? fail(error) : done()),
    );
  });
}
