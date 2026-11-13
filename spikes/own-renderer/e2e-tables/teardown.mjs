// Track `durable-tables`: once the run's hosts are stopped, the schemas the
// guestbook's spec ran on dropped (`schemas.mjs`).
import { ENGINES } from "../host-metadata.mjs";
import { dropped, schemaOf } from "./schemas.mjs";

export default async function teardown() {
  for (const engine of ENGINES) dropped(schemaOf(engine));
}
