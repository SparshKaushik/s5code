import { remoteCacheUrl } from "./remoteCache.ts";

export default function start({ root }: { readonly root: string }) {
  const url = remoteCacheUrl(root);
  if (!url) return;
  const finish = async () => {};
  return Object.assign(finish, {
    remoteCache: process.env.VP_REMOTE_CACHE || process.env.VP_T3_TASK_CACHE_MODE || "read-write",
  });
}
