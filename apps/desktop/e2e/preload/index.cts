import { contextBridge, ipcRenderer } from 'electron';

import '../../preload/index.cjs';

const MAIN_ROUTE = 'aura:e2e:p08-contract-driver';

contextBridge.exposeInMainWorld(
  'auraP08ContractDriver',
  Object.freeze({
    run(probe: 'invalid_identity' | 'invalid_token' | 'valid' = 'valid'): Promise<unknown> {
      return ipcRenderer.invoke(MAIN_ROUTE, { probe });
    },
  }),
);
