#!/usr/bin/env bash
# Bob's owned put of his own row, while another transaction deletes his row
# and inserts Alice's at the same key (uncommitted while his statement
# waits). Prints what the statement answers (count, old_held, pw_mine,
# had_room), and the table after. One scratch schema, dropped at the end.
set -u
source ~/pw-setup/env.sh >/dev/null 2>&1
DB="$PW_STORE_DATABASE_URL"
ISO="${1:-READ COMMITTED}"
S="pr91_replaced_$$"

psql -X -q "$DB" -v ON_ERROR_STOP=1 <<SQL
CREATE SCHEMA $S;
CREATE TABLE $S.notes (shelf text, id text, slug text, text text, "pw:owner" text,
                       PRIMARY KEY (shelf, id));
CREATE UNIQUE INDEX ON $S.notes (slug);
INSERT INTO $S.notes VALUES ('home', 'milk', 'm', 'his', 's-bob');
SQL

psql -X -q "$DB" <<SQL &
BEGIN ISOLATION LEVEL $ISO;
DELETE FROM $S.notes WHERE shelf = 'home' AND id = 'milk';
INSERT INTO $S.notes VALUES ('home', 'milk', 'm', 'hers', 's-alice');
SELECT pg_sleep(3);
COMMIT;
SQL

psql -X -q -A -F ' ' "$DB" <<SQL
SELECT pg_sleep(1);
BEGIN ISOLATION LEVEL $ISO;
WITH holder AS (
  SELECT t.shelf, t.id, t.slug, t.text, CASE WHEN t.slug = 'b' THEN 0 END AS pw_rank
    FROM $S.notes t WHERE (t.slug = 'b') AND NOT (t.shelf = 'home' AND t.id = 'milk')
   ORDER BY pw_rank LIMIT 1),
old AS (
  SELECT t.shelf, t.id, t.slug, t.text, 1 AS pw_old,
         (false OR COALESCE(t."pw:owner" = 's-bob', false)) AS pw_mine
    FROM $S.notes t WHERE t.shelf = 'home' AND t.id = 'milk'),
written AS (
  INSERT INTO $S.notes AS pw_t (shelf, id, slug, text, "pw:owner")
  SELECT 'home', 'milk', 'b', 'changed', 's-bob'
   WHERE NOT EXISTS (SELECT 1 FROM holder)
  ON CONFLICT (shelf, id) DO UPDATE SET slug = EXCLUDED.slug, text = EXCLUDED.text
   WHERE (false OR COALESCE(pw_t."pw:owner" = 's-bob', false))
  RETURNING 1)
SELECT 'answer:', (SELECT count(*) FROM written) AS count, h.pw_rank AS holder,
       o.pw_old IS NOT NULL AS old_held, COALESCE(o.pw_mine, false) AS pw_mine, true AS had_room
  FROM (VALUES (1)) AS one LEFT JOIN holder h ON true LEFT JOIN old o ON true;
COMMIT;
SQL
wait

echo "after ($ISO):"
psql -X -q -A -F ' ' "$DB" -c "SELECT shelf, id, slug, text, \"pw:owner\" FROM $S.notes"
psql -X -q "$DB" -c "DROP SCHEMA $S CASCADE"
