// Track `durable-tables`: the PostgreSQL schemas a run of
// `playwright.tables.config.mjs` keeps the guestbook's tables in, one per
// engine, named by the run (`PW_E2E_TABLES_RUN`, which the config sets once,
// in the process that starts the hosts) and by `PORT`, so that two runs on
// one database never share one; and each dropped where `psql` can drop it.
import { execFileSync } from "node:child_process";
import { PORT } from "../host-metadata.mjs";

/** The schema `engine`'s hosts of this run keep the tables in. */
export const schemaOf = (engine) => `pw_e2e_tables_${PORT}_${engine}_${process.env.PW_E2E_TABLES_RUN}`;

/** `schema` dropped, where `psql` can; one left behind holds a run's entries
 * and nothing else. */
export function dropped(schema) {
  try {
    execFileSync("psql", [process.env.PW_TABLES_DATABASE_URL ?? "", "-qAtc", `DROP SCHEMA IF EXISTS ${schema} CASCADE`], {
      stdio: "ignore",
    });
  } catch {
    console.warn(`guestbook on PostgreSQL: the schema ${schema} was not dropped (psql)`);
  }
}
