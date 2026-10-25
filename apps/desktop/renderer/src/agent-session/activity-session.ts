import { createContext, useContext } from 'react';

/** The conversation a run of tool activity belongs to, for rows that read its records. */
export const ActivitySession = createContext<string | undefined>(undefined);

export const useActivitySession = (): string | undefined => useContext(ActivitySession);
