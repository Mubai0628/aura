import { resolve } from 'node:path';
import { checkDataLocations } from './data-locations.mjs';

checkDataLocations(resolve(import.meta.dirname, '../..'));
