CREATE TABLE IF NOT EXISTS "relay_android_live_updates" (
  "user_id" varchar(255) NOT NULL,
  "device_id" varchar(255) NOT NULL,
  "generation_id" varchar(255) NOT NULL,
  "armed_at" varchar(64) NOT NULL,
  "last_aggregate_json" jsonb,
  "last_delivery_at" varchar(64),
  "created_at" varchar(64) NOT NULL,
  "updated_at" varchar(64) NOT NULL,
  CONSTRAINT "relay_android_live_updates_user_id_device_id_pk" PRIMARY KEY("user_id", "device_id")
);
