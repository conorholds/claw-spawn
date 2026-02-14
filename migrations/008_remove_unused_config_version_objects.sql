-- F-009: Remove obsolete config version objects kept from initial design.
-- Active code uses get_next_config_version_atomic(UUID).

DROP FUNCTION IF EXISTS get_next_config_version(UUID);
DROP SEQUENCE IF EXISTS bot_config_version_seq;
