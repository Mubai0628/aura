/**
 * Empty answers for every read route the session grant dialog loads when it opens.
 *
 * Tests that open "会话授权管理" only on the way to something else, such as the
 * desktop shell's permission menu, still have to answer each route the dialog
 * really calls. A missing route makes the dialog crash instead of exercising the
 * behaviour under test, so a new dialog section adds its route here once.
 */
export const emptySessionGrantRoutes = {
  listExternalGrants: async () => ({ ok: true, value: [] }),
  listNativeSessionConsents: async () => ({ ok: true, value: [] }),
  listFileSessionConsents: async () => ({ ok: true, value: [] }),
  listMemorySessionConsents: async () => ({ ok: true, value: [] }),
  listNetworkSessionConsents: async () => ({ ok: true, value: [] }),
};
