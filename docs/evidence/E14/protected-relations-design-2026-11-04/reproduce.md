# Relation-window SQL design probe

Run `source /home/haki/pw-setup/env.sh`, then the archived probe runner with a
throwaway `PW_STORE_DATABASE_URL`. The runner retains the original local helper
path; alternatively pass this directory's `primary-probe.sql` to the same psql
command. It creates TEMP fixtures in a transaction and rolls back. This is SQL
semantics evidence only; no generic relation implementation existed for it.
The exact original SQL and runner are retained, with no credentials.
