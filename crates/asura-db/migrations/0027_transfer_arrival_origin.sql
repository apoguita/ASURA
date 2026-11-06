-- A doorway arrival is a property of this transfer, never inferred from saved coordinates.
ALTER TABLE t_transfer ADD COLUMN IF NOT EXISTS arrival_from_map_id integer;
INSERT INTO schema_version (version) VALUES (27) ON CONFLICT (version) DO NOTHING;
