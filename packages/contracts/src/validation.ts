import Ajv2020, { type AnySchema, type ValidateFunction } from 'ajv/dist/2020.js';
import addFormats from 'ajv-formats';

const UINT8_MAX = 255;
const UINT16_MAX = 65_535;
const UINT32_MAX = 4_294_967_295;

export const createContractValidator = <Contract = unknown>(
  schema: AnySchema,
): ValidateFunction<Contract> => {
  const ajv = new Ajv2020({ allErrors: true, strict: true });
  addFormats(ajv);
  ajv.addFormat('uint8', {
    type: 'number',
    validate: (value: number) => Number.isInteger(value) && value >= 0 && value <= UINT8_MAX,
  });
  ajv.addFormat('uint16', {
    type: 'number',
    validate: (value: number) => Number.isInteger(value) && value >= 0 && value <= UINT16_MAX,
  });
  ajv.addFormat('uint32', {
    type: 'number',
    validate: (value: number) => Number.isInteger(value) && value >= 0 && value <= UINT32_MAX,
  });
  ajv.addFormat('uint64', {
    type: 'number',
    validate: (value: number) => Number.isSafeInteger(value) && value >= 0,
  });
  return ajv.compile<Contract>(schema);
};
