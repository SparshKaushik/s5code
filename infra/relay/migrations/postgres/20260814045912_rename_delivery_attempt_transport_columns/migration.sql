ALTER TABLE "relay_delivery_attempts" RENAME COLUMN "apns_status" TO "delivery_status";
ALTER TABLE "relay_delivery_attempts" RENAME COLUMN "apns_reason" TO "delivery_reason";
ALTER TABLE "relay_delivery_attempts" RENAME COLUMN "apns_id" TO "provider_message_id";
