import { createContext } from 'react';

/**
 * The run a 停止 was asked for and has not ended yet. Core answers only once the run has really
 * stopped, which can take a moment; the page says it is stopping from the click on.
 */
export const StoppingRunContext = createContext<string | undefined>(undefined);
