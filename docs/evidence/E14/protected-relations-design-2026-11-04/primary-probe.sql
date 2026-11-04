SHOW server_version;
BEGIN;
CREATE TEMP TABLE relation_members(reader bytea, topic bigint, author bytea, edge bigint);
CREATE TEMP TABLE relation_posts(id bytea PRIMARY KEY, author bytea, seq bigint);
INSERT INTO relation_members VALUES
 ('\x616c696365',0,'\x61',1),('\x616c696365',0,'\x61',2),
 ('\x616c696365',0,'\x62',3),('\x626f62',0,'\x63',4),
 ('\x616c696365',1,'\x61',5),('\x616c696365',2,'\x61',6);
INSERT INTO relation_posts VALUES
 ('\x6131','\x61',1),('\x6133','\x61',3),
 ('\x6232','\x62',2),('\x6233','\x62',3),('\x6339','\x63',9);
CREATE TEMP VIEW relation_scalar AS
 SELECT p.* FROM relation_posts p WHERE EXISTS
 (SELECT 1 FROM relation_members m WHERE m.reader='\x616c696365'::bytea
  AND m.topic=0 AND m.author=p.author)
 ORDER BY p.seq DESC,p.id DESC LIMIT 2;
CREATE TEMP VIEW relation_set AS
 SELECT k.slot,p.id,p.seq FROM
 (VALUES (1::bigint,'\x616c696365'::bytea,0::bigint),
         (2::bigint,'\x626f62'::bytea,0::bigint),
         (3::bigint,'\x6d697373696e67'::bytea,0::bigint),
         (4::bigint,'\x616c696365'::bytea,1::bigint),
         (5::bigint,'\x616c696365'::bytea,2::bigint)) k(slot,reader,topic)
 CROSS JOIN LATERAL
 (SELECT t.* FROM relation_posts t WHERE EXISTS
  (SELECT 1 FROM relation_members m WHERE m.reader=k.reader AND m.topic=k.topic
   AND m.author=t.author)
  ORDER BY t.seq DESC,t.id DESC LIMIT 2) p
 ORDER BY k.slot,p.seq DESC,p.id DESC;
DO $$ BEGIN
 IF (SELECT array_agg(convert_from(id,'UTF8')) FROM relation_scalar) != ARRAY['b3','a3']
 THEN RAISE EXCEPTION 'global limit or complete descending tie-break failed'; END IF;
 IF (SELECT count(*) FROM relation_set WHERE slot=1) != 2
 OR (SELECT count(*) FROM relation_set WHERE slot=2) != 1
 OR (SELECT count(*) FROM relation_set WHERE slot=3) != 0
 OR (SELECT count(*) FROM relation_set WHERE slot IN (4,5) AND id='\x6133') != 2
 THEN RAISE EXCEPTION 'per-selector limit, missing group or overlap failed'; END IF;
END $$;
SELECT 'scalar global window',string_agg(convert_from(id,'UTF8'),',' ORDER BY seq DESC,id DESC)
FROM relation_scalar;
SELECT slot,string_agg(convert_from(id,'UTF8'),',' ORDER BY seq DESC,id DESC)
FROM relation_set GROUP BY slot ORDER BY slot;
SELECT 'logical target rows for alice with duplicate edges',count(*) FROM relation_scalar;
ROLLBACK;
