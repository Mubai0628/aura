import { expect, test } from 'vitest';
import { FILES_TREE_MIN_WIDTH, FILES_VIEW_MIN_WIDTH, filesTreeMaxWidth } from './files-tree-width';

// Dragged as far as it goes, the tree leaves the open file its 240 px beside the 7 px
// line between them (the E2E measured 233 px when the line was not counted).
test('the widest tree leaves the open file its whole minimum beside the line', () => {
  expect(filesTreeMaxWidth(900)).toBe(900 - FILES_VIEW_MIN_WIDTH - 7);
  expect(filesTreeMaxWidth(300)).toBe(FILES_TREE_MIN_WIDTH);
});
