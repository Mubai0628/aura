import { execFile } from 'node:child_process';

/** The identity an installation's notifications are sent under; setup removes the same. */
export const notificationIdentity = (namespace: string): string => `Aura.${namespace}`;

const setValue = (identity: string, name: string, value: string): Promise<void> =>
  new Promise((done, fail) => {
    execFile(
      'reg.exe',
      [
        'add',
        `HKCU\\Software\\Classes\\AppUserModelId\\${identity}`,
        '/v',
        name,
        '/t',
        'REG_SZ',
        '/d',
        value,
        '/f',
      ],
      { windowsHide: true },
      (error) => (error ? fail(error) : done()),
    );
  });

/**
 * Names the identity for Windows (HKCU\Software\Classes\AppUserModelId\<identity>), so a
 * notification is headed "Aura", with Aura's icon when there is one, rather than with the identity
 * itself. Uninstalling removes it.
 */
export async function registerNotificationIdentity(identity: string, icon?: string): Promise<void> {
  if (!/^Aura\.[a-z0-9][a-z0-9.-]*$/u.test(identity))
    throw new Error('notification identity is not valid');
  await setValue(identity, 'DisplayName', 'Aura');
  if (icon) await setValue(identity, 'IconUri', icon);
}
