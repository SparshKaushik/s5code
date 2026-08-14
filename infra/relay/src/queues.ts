import * as Cloudflare from "alchemy/Cloudflare";

export const RelayDeliveryDeadLetterQueue = Cloudflare.Queues.Queue("RelayDeliveryDeadLetterQueue");

export const RelayDeliveryQueue = Cloudflare.Queues.Queue("RelayDeliveryQueue");
