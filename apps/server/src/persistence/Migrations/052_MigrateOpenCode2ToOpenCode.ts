import * as Effect from "effect/Effect";
import * as SqlClient from "effect/unstable/sql/SqlClient";

export default Effect.gen(function* () {
  const sql = yield* SqlClient.SqlClient;

  // 1. projection_threads
  yield* sql`
    UPDATE projection_threads
    SET model_selection_json = CASE
      WHEN json_extract(model_selection_json, '$.instanceId') = 'opencode2'
           AND json_extract(model_selection_json, '$.provider') = 'opencode2'
      THEN json_set(json_set(model_selection_json, '$.instanceId', 'opencode'), '$.provider', 'opencode')
      WHEN json_extract(model_selection_json, '$.instanceId') = 'opencode2'
      THEN json_set(model_selection_json, '$.instanceId', 'opencode')
      WHEN json_extract(model_selection_json, '$.provider') = 'opencode2'
      THEN json_set(model_selection_json, '$.provider', 'opencode')
      ELSE model_selection_json
    END
    WHERE json_extract(model_selection_json, '$.provider') = 'opencode2'
       OR json_extract(model_selection_json, '$.instanceId') = 'opencode2'
  `;

  yield* sql`
    UPDATE projection_threads
    SET model_selection_json = json_set(
      model_selection_json,
      '$.model',
      'opencode/' || substr(json_extract(model_selection_json, '$.model'), 11)
    )
    WHERE json_extract(model_selection_json, '$.model') LIKE 'opencode2/%'
  `;

  // 2. projection_projects
  yield* sql`
    UPDATE projection_projects
    SET default_model_selection_json = CASE
      WHEN json_extract(default_model_selection_json, '$.instanceId') = 'opencode2'
           AND json_extract(default_model_selection_json, '$.provider') = 'opencode2'
      THEN json_set(json_set(default_model_selection_json, '$.instanceId', 'opencode'), '$.provider', 'opencode')
      WHEN json_extract(default_model_selection_json, '$.instanceId') = 'opencode2'
      THEN json_set(default_model_selection_json, '$.instanceId', 'opencode')
      WHEN json_extract(default_model_selection_json, '$.provider') = 'opencode2'
      THEN json_set(default_model_selection_json, '$.provider', 'opencode')
      ELSE default_model_selection_json
    END
    WHERE json_extract(default_model_selection_json, '$.provider') = 'opencode2'
       OR json_extract(default_model_selection_json, '$.instanceId') = 'opencode2'
  `;

  yield* sql`
    UPDATE projection_projects
    SET default_model_selection_json = json_set(
      default_model_selection_json,
      '$.model',
      'opencode/' || substr(json_extract(default_model_selection_json, '$.model'), 11)
    )
    WHERE json_extract(default_model_selection_json, '$.model') LIKE 'opencode2/%'
  `;

  // 3. projection_thread_sessions
  yield* sql`
    UPDATE projection_thread_sessions
    SET provider_name = CASE WHEN provider_name = 'opencode2' THEN 'opencode' ELSE provider_name END,
        provider_instance_id = CASE WHEN provider_instance_id = 'opencode2' THEN 'opencode' ELSE provider_instance_id END
    WHERE provider_name = 'opencode2' OR provider_instance_id = 'opencode2'
  `;

  // 4. provider_session_runtime
  yield* sql`
    UPDATE provider_session_runtime
    SET provider_name = CASE WHEN provider_name = 'opencode2' THEN 'opencode' ELSE provider_name END,
        provider_instance_id = CASE WHEN provider_instance_id = 'opencode2' THEN 'opencode' ELSE provider_instance_id END
    WHERE provider_name = 'opencode2' OR provider_instance_id = 'opencode2'
  `;

  // 5. orchestration_events - thread modelSelection
  yield* sql`
    UPDATE orchestration_events
    SET payload_json = CASE
      WHEN json_extract(payload_json, '$.modelSelection.instanceId') = 'opencode2'
           AND json_extract(payload_json, '$.modelSelection.provider') = 'opencode2'
      THEN json_set(json_set(payload_json, '$.modelSelection.instanceId', 'opencode'), '$.modelSelection.provider', 'opencode')
      WHEN json_extract(payload_json, '$.modelSelection.instanceId') = 'opencode2'
      THEN json_set(payload_json, '$.modelSelection.instanceId', 'opencode')
      WHEN json_extract(payload_json, '$.modelSelection.provider') = 'opencode2'
      THEN json_set(payload_json, '$.modelSelection.provider', 'opencode')
      ELSE payload_json
    END
    WHERE event_type IN ('thread.created', 'thread.meta-updated', 'thread.turn-start-requested')
      AND (json_extract(payload_json, '$.modelSelection.provider') = 'opencode2'
        OR json_extract(payload_json, '$.modelSelection.instanceId') = 'opencode2')
  `;

  yield* sql`
    UPDATE orchestration_events
    SET payload_json = json_set(
      payload_json,
      '$.modelSelection.model',
      'opencode/' || substr(json_extract(payload_json, '$.modelSelection.model'), 11)
    )
    WHERE event_type IN ('thread.created', 'thread.meta-updated', 'thread.turn-start-requested')
      AND json_extract(payload_json, '$.modelSelection.model') LIKE 'opencode2/%'
  `;

  // 6. orchestration_events - project defaultModelSelection
  yield* sql`
    UPDATE orchestration_events
    SET payload_json = CASE
      WHEN json_extract(payload_json, '$.defaultModelSelection.instanceId') = 'opencode2'
           AND json_extract(payload_json, '$.defaultModelSelection.provider') = 'opencode2'
      THEN json_set(json_set(payload_json, '$.defaultModelSelection.instanceId', 'opencode'), '$.defaultModelSelection.provider', 'opencode')
      WHEN json_extract(payload_json, '$.defaultModelSelection.instanceId') = 'opencode2'
      THEN json_set(payload_json, '$.defaultModelSelection.instanceId', 'opencode')
      WHEN json_extract(payload_json, '$.defaultModelSelection.provider') = 'opencode2'
      THEN json_set(payload_json, '$.defaultModelSelection.provider', 'opencode')
      ELSE payload_json
    END
    WHERE event_type IN ('project.created', 'project.meta-updated')
      AND (json_extract(payload_json, '$.defaultModelSelection.provider') = 'opencode2'
        OR json_extract(payload_json, '$.defaultModelSelection.instanceId') = 'opencode2')
  `;

  yield* sql`
    UPDATE orchestration_events
    SET payload_json = json_set(
      payload_json,
      '$.defaultModelSelection.model',
      'opencode/' || substr(json_extract(payload_json, '$.defaultModelSelection.model'), 11)
    )
    WHERE event_type IN ('project.created', 'project.meta-updated')
      AND json_extract(payload_json, '$.defaultModelSelection.model') LIKE 'opencode2/%'
  `;

  // 7. orchestration_events - thread.session-set
  yield* sql`
    UPDATE orchestration_events
    SET payload_json = CASE
      WHEN json_extract(payload_json, '$.session.providerInstanceId') = 'opencode2'
           AND json_extract(payload_json, '$.session.providerName') = 'opencode2'
      THEN json_set(json_set(payload_json, '$.session.providerInstanceId', 'opencode'), '$.session.providerName', 'opencode')
      WHEN json_extract(payload_json, '$.session.providerInstanceId') = 'opencode2'
      THEN json_set(payload_json, '$.session.providerInstanceId', 'opencode')
      WHEN json_extract(payload_json, '$.session.providerName') = 'opencode2'
      THEN json_set(payload_json, '$.session.providerName', 'opencode')
      ELSE payload_json
    END
    WHERE event_type = 'thread.session-set'
      AND (json_extract(payload_json, '$.session.providerName') = 'opencode2'
        OR json_extract(payload_json, '$.session.providerInstanceId') = 'opencode2')
  `;
});
