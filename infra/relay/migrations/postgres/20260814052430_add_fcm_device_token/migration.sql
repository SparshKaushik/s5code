ALTER TABLE "relay_mobile_devices" ALTER COLUMN "ios_major_version" DROP NOT NULL;
ALTER TABLE "relay_mobile_devices" ADD COLUMN "fcm_token" text;
CREATE UNIQUE INDEX "idx_relay_mobile_devices_fcm_token" ON "relay_mobile_devices" ("fcm_token");
