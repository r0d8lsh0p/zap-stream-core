-- Migrate Cloudflare Live Input UIDs from stream_key to external_id.
-- The old zap-stream core binary stored CF UIDs in user.stream_key.
-- The new zap-stream-external binary expects them in user.external_id.
-- CF UIDs are 32-character lowercase hex strings.
UPDATE user
SET external_id = stream_key
WHERE external_id IS NULL
  AND LENGTH(stream_key) = 32
  AND stream_key REGEXP '^[0-9a-f]{32}$';
