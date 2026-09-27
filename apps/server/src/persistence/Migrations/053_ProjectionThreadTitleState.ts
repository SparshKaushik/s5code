import * as Effect from "effect/Effect";
import * as SqlClient from "effect/unstable/sql/SqlClient";

export default Effect.gen(function* () {
  const sql = yield* SqlClient.SqlClient;
  // Guarded because databases adopted from upstream already recorded this
  // migration under its original id (52); the fork renumbered it to 053 after
  // inserting MigrateOpenCode2ToOpenCode at 052, so those databases run it again.
  const columns = yield* sql<{ readonly name: string }>`
    SELECT name FROM pragma_table_info('projection_threads') WHERE name = 'title_state_json'
  `;
  if (columns.length === 0) {
    yield* sql`ALTER TABLE projection_threads ADD COLUMN title_state_json TEXT`;
  }
});
