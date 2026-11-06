import { expect, test } from 'vitest';
import { parseProjection } from '../projection-validation.js';

// The model settings may list the speed modes each model offers, with their prices; anything
// else in that place is refused, as any other unknown field is.

const MODEL_ID = 'mdl_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const SECOND_MODEL_ID = 'mdl_01ARZ3NDEKTSV4RRFFQ69G5FAW';

const change = (speedModes: unknown) => ({
  payload: {
    active_credentials: [],
    aura_revision: '3',
    default_model_profile_id: null,
    dormant_credentials: [],
    models_revision: '8',
    registry: {},
    speed_modes: speedModes,
  },
  projection_kind: 'model_settings',
  projection_scope: { type: 'global' },
  revision: '9',
  runtime_instance_id: 'rnd_01ARZ3NDEKTSV4RRFFQ69G5FAV',
});

const price = (fields: Record<string, unknown> = {}) => ({
  cache_read: '0.2',
  cache_write: null,
  currency: 'usd',
  input: '2',
  output: '10',
  unit: 'per_million_tokens',
  ...fields,
});

const model = (fields: Record<string, unknown> = {}) => ({
  model_profile_id: MODEL_ID,
  modes: [{ name: 'fast', price: price({ input: '4', output: '20' }) }],
  standard_price: price(),
  ...fields,
});

test('the settings list the modes of each model, with their prices or none', () => {
  for (const speedModes of [
    undefined,
    [],
    [model()],
    [model(), model({ model_profile_id: SECOND_MODEL_ID })],
    [model({ standard_price: null })],
    [model({ modes: [{ name: 'flex', price: null }, { name: 'priority' }] })],
    [model({ modes: [{ name: 'fast_2.v-1', price: price({ cache_read: undefined }) }] })],
    [model({ standard_price: price({ input: null, output: null, cache_read: null }) })],
  ])
    expect(parseProjection(change(speedModes)), JSON.stringify(speedModes)).toBeDefined();
});

test('a malformed mode, price or model is refused', () => {
  for (const speedModes of [
    {},
    [{ ...model(), extra: true }],
    [model({ model_profile_id: 'not-an-id' })],
    [model({ modes: undefined })],
    [model({ modes: {} })],
    [model(), model()],
    [model({ modes: [{ name: 'fast' }, { name: 'fast' }] })],
    [model({ modes: [{ name: 'fast', extra: true }] })],
    [model({ modes: [{ price: null }] })],
    [model({ modes: [{ name: '' }] })],
    [model({ modes: [{ name: 'a'.repeat(65) }] })],
    [model({ modes: [{ name: 'odd name' }] })],
    [model({ modes: [{ name: '快速' }] })],
    [model({ modes: [{ name: 7 }] })],
    [model({ standard_price: price({ currency: 'eur' }) })],
    [model({ standard_price: price({ unit: 'per_token' }) })],
    [model({ standard_price: price({ extra: '1' }) })],
    [model({ standard_price: { input: '1' } })],
    [model({ standard_price: price({ input: '-1' }) })],
    [model({ standard_price: price({ output: '1.50' }) })],
    [model({ standard_price: price({ cache_read: '01' }) })],
    [model({ standard_price: price({ cache_write: 1 }) })],
    [model({ modes: [{ name: 'fast', price: price({ input: '1e3' }) }] })],
    [model({ modes: [{ name: 'fast', price: 'free' }] })],
  ])
    expect(parseProjection(change(speedModes)), JSON.stringify(speedModes)).toBeUndefined();
});
