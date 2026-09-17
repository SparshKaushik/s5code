import { assert, it } from "@effect/vitest";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as Schema from "effect/Schema";
import * as SqlClient from "effect/unstable/sql/SqlClient";

import { runMigrations } from "../Migrations.ts";
import * as NodeSqliteClient from "@t3tools/shared/nodeSqliteClient";

const decodeJson = Schema.decodeUnknownSync(Schema.fromJsonString(Schema.Unknown));

const layer = it.layer(Layer.mergeAll(NodeSqliteClient.layerMemory()));

layer("052_MigrateOpenCode2ToOpenCode", (it) => {
  it.effect("migrates opencode2 references to opencode across all tables and events", () =>
    Effect.gen(function* () {
      const sql = yield* SqlClient.SqlClient;

      yield* runMigrations({ toMigrationInclusive: 51 });

      // Insert thread with opencode2 modelSelection
      yield* sql`
        INSERT INTO projection_threads (
          thread_id,
          project_id,
          title,
          model_selection_json,
          branch,
          worktree_path,
          latest_turn_id,
          created_at,
          updated_at,
          deleted_at
        ) VALUES (
          'thread-1',
          'project-1',
          'OpenCode 2 thread',
          '{"instanceId":"opencode2","model":"opencode2/gpt-5","options":[]}',
          'main',
          '/tmp/repo',
          NULL,
          '2026-01-01T00:00:00.000Z',
          '2026-01-01T00:00:00.000Z',
          NULL
        ), (
          'thread-2',
          'project-1',
          'OpenCode legacy provider thread',
          '{"provider":"opencode2","model":"opencode2/claude-3-7","options":[]}',
          'main',
          '/tmp/repo',
          NULL,
          '2026-01-01T00:00:00.000Z',
          '2026-01-01T00:00:00.000Z',
          NULL
        ), (
          'thread-3',
          'project-1',
          'Codex thread',
          '{"instanceId":"codex","model":"gpt-5.4","options":[]}',
          'main',
          '/tmp/repo',
          NULL,
          '2026-01-01T00:00:00.000Z',
          '2026-01-01T00:00:00.000Z',
          NULL
        )
      `;

      // Insert project with opencode2 default_model_selection_json
      yield* sql`
        INSERT INTO projection_projects (
          project_id,
          title,
          workspace_root,
          default_model_selection_json,
          scripts_json,
          created_at,
          updated_at,
          deleted_at
        ) VALUES (
          'project-1',
          'Project with opencode2 default',
          '/tmp/project',
          '{"instanceId":"opencode2","model":"opencode2/claude-3-7","options":[]}',
          '[]',
          '2026-01-01T00:00:00.000Z',
          '2026-01-01T00:00:00.000Z',
          NULL
        )
      `;

      // Insert projection_thread_sessions
      yield* sql`
        INSERT INTO projection_thread_sessions (
          thread_id,
          status,
          provider_name,
          provider_instance_id,
          provider_session_id,
          provider_thread_id,
          active_turn_id,
          last_error,
          updated_at
        ) VALUES (
          'thread-1',
          'idle',
          'opencode2',
          'opencode2',
          'session-1',
          't-1',
          NULL,
          NULL,
          '2026-01-01T00:00:00.000Z'
        ), (
          'thread-3',
          'idle',
          'codex',
          'codex',
          'session-3',
          't-3',
          NULL,
          NULL,
          '2026-01-01T00:00:00.000Z'
        )
      `;

      // Insert provider_session_runtime
      yield* sql`
        INSERT INTO provider_session_runtime (
          thread_id,
          provider_name,
          provider_instance_id,
          adapter_key,
          runtime_mode,
          status,
          last_seen_at
        ) VALUES (
          'thread-1',
          'opencode2',
          'opencode2',
          'adapter-1',
          'full-access',
          'idle',
          '2026-01-01T00:00:00.000Z'
        )
      `;

      // Insert orchestration_events
      yield* sql`
        INSERT INTO orchestration_events (
          event_id,
          aggregate_kind,
          stream_id,
          stream_version,
          sequence,
          event_type,
          actor_kind,
          command_id,
          correlation_id,
          occurred_at,
          payload_json,
          metadata_json
        ) VALUES (
          'ev-1',
          'thread',
          'thread-1',
          1,
          1,
          'thread.created',
          'client',
          'cmd-1',
          'corr-1',
          '2026-01-01T00:00:00.000Z',
          '{"threadId":"thread-1","modelSelection":{"instanceId":"opencode2","model":"opencode2/gpt-5"}}',
          '{}'
        ), (
          'ev-2',
          'thread',
          'thread-1',
          2,
          2,
          'thread.session-set',
          'server',
          'cmd-2',
          'corr-2',
          '2026-01-01T00:00:00.000Z',
          '{"threadId":"thread-1","session":{"providerName":"opencode2","providerInstanceId":"opencode2","status":"idle"}}',
          '{}'
        ), (
          'ev-3',
          'project',
          'project-1',
          1,
          3,
          'project.created',
          'client',
          'cmd-3',
          'corr-3',
          '2026-01-01T00:00:00.000Z',
          '{"projectId":"project-1","defaultModelSelection":{"instanceId":"opencode2","model":"opencode2/claude-3-7"}}',
          '{}'
        )
      `;

      // Run migration 52
      yield* runMigrations({ toMigrationInclusive: 52 });

      // Assert projection_threads migrated
      const threads = yield* sql<{
        readonly thread_id: string;
        readonly model_selection_json: string;
      }>`
        SELECT thread_id, model_selection_json
        FROM projection_threads
        ORDER BY thread_id
      `;
      assert.equal(threads.length, 3);

      const thread1 = decodeJson(threads[0]!.model_selection_json) as {
        readonly instanceId: string;
        readonly model: string;
      };
      assert.equal(thread1.instanceId, "opencode");
      assert.equal(thread1.model, "opencode/gpt-5");

      const thread2 = decodeJson(threads[1]!.model_selection_json) as {
        readonly provider: string;
        readonly model: string;
      };
      assert.equal(thread2.provider, "opencode");
      assert.equal(thread2.model, "opencode/claude-3-7");

      const thread3 = decodeJson(threads[2]!.model_selection_json) as {
        readonly instanceId: string;
        readonly model: string;
      };
      assert.equal(thread3.instanceId, "codex");
      assert.equal(thread3.model, "gpt-5.4");

      // Assert projection_projects migrated
      const projects = yield* sql<{ readonly default_model_selection_json: string }>`
        SELECT default_model_selection_json
        FROM projection_projects
      `;
      assert.equal(projects.length, 1);
      const project1 = decodeJson(projects[0]!.default_model_selection_json) as {
        readonly instanceId: string;
        readonly model: string;
      };
      assert.equal(project1.instanceId, "opencode");
      assert.equal(project1.model, "opencode/claude-3-7");

      // Assert projection_thread_sessions migrated
      const sessions = yield* sql<{
        readonly thread_id: string;
        readonly provider_name: string;
        readonly provider_instance_id: string;
      }>`
        SELECT thread_id, provider_name, provider_instance_id
        FROM projection_thread_sessions
        ORDER BY thread_id
      `;
      assert.equal(sessions.length, 2);
      assert.equal(sessions[0]!.thread_id, "thread-1");
      assert.equal(sessions[0]!.provider_name, "opencode");
      assert.equal(sessions[0]!.provider_instance_id, "opencode");
      assert.equal(sessions[1]!.thread_id, "thread-3");
      assert.equal(sessions[1]!.provider_name, "codex");
      assert.equal(sessions[1]!.provider_instance_id, "codex");

      // Assert provider_session_runtime migrated
      const runtime = yield* sql<{
        readonly provider_name: string;
        readonly provider_instance_id: string;
      }>`
        SELECT provider_name, provider_instance_id
        FROM provider_session_runtime
      `;
      assert.equal(runtime.length, 1);
      assert.equal(runtime[0]!.provider_name, "opencode");
      assert.equal(runtime[0]!.provider_instance_id, "opencode");

      // Assert orchestration_events migrated
      const events = yield* sql<{ readonly event_type: string; readonly payload_json: string }>`
        SELECT event_type, payload_json
        FROM orchestration_events
        ORDER BY sequence
      `;
      assert.equal(events.length, 3);
      const event1 = decodeJson(events[0]!.payload_json) as {
        readonly modelSelection: { readonly instanceId: string; readonly model: string };
      };
      assert.equal(event1.modelSelection.instanceId, "opencode");
      assert.equal(event1.modelSelection.model, "opencode/gpt-5");

      const event2 = decodeJson(events[1]!.payload_json) as {
        readonly session: {
          readonly providerName: string;
          readonly providerInstanceId: string;
        };
      };
      assert.equal(event2.session.providerName, "opencode");
      assert.equal(event2.session.providerInstanceId, "opencode");

      const event3 = decodeJson(events[2]!.payload_json) as {
        readonly defaultModelSelection: {
          readonly instanceId: string;
          readonly model: string;
        };
      };
      assert.equal(event3.defaultModelSelection.instanceId, "opencode");
      assert.equal(event3.defaultModelSelection.model, "opencode/claude-3-7");
    }),
  );
});
