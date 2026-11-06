import type { ComputerControlApiV1 } from '../../../shared/computer-control-api';

/** The preload's computer control bridge, when this window has one. */
export const computerControl = (): ComputerControlApiV1 | undefined => window.aura?.v1.computer;
