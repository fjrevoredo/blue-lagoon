UPDATE schema_migrations
SET name = 'runtime_foundation'
WHERE version = 1;

UPDATE schema_migrations
SET name = 'foreground_loop'
WHERE version = 2;
