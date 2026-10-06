import { defineConfig } from "@coderabbitai/config";

export default defineConfig({
  language: "en-US",
  early_access: false,
  reviews: {
    profile: "chill",
    request_changes_workflow: false,
    high_level_summary: true,
    poem: false,
    review_status: true,
    collapse_walkthrough: false,
    auto_review: {
      enabled: true,
      auto_incremental_review: true,
      drafts: false,
      base_branches: ["main"],
      ignore_title_keywords: ["WIP", "[skip review]", "[ci-skip]"],
    },
    pre_merge_checks: {
      docstrings: { mode: "off" },
    },
    path_filters: [
      // Vendored read-only reference checkouts of upstream Effect and Alchemy
      // (see scripts/lib/reference-repos.ts). Nothing imports from them.
      "!**/.repos/**",
      "!**/.t3/**",
      "!**/pnpm-lock.yaml",
      "!**/node_modules/**",
    ],
    path_instructions: [
      {
        path: "**/*",
        instructions:
          "Follow AGENTS.md. Complexity belongs at the adapter boundary; orchestration stays pure and UI stays dumb. Prefer inferred types and never introduce any. Flag continuously repainting animations and unnecessary WebSocket payloads. Avoid formatting nitpicks handled by CI; focus on correctness, data loss, and security. Be concise and actionable.",
      },
      {
        path: "{apps,packages,infra}/**/*.ts",
        instructions: "Hold changed code to the rules in docs/internals/effect-services.md.",
      },
      {
        path: "apps/web/src/**/*.{tsx,css}",
        instructions: "Hold changed code to the rules in docs/internals/web-ui.md.",
      },
    ],
  },
  chat: { auto_reply: true },
  knowledge_base: {
    code_guidelines: {
      filePatterns: [
        { files: "docs/internals/effect-services.md", applyTo: "{apps,packages,infra}/**/*.ts" },
        { files: "docs/internals/web-ui.md", applyTo: "apps/web/src/**/*.{tsx,css}" },
      ],
    },
  },
});
