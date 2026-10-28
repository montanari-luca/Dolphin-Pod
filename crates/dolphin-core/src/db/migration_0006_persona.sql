-- v6: editable per-agent persona (voice and character), layered on top of the built-in specialty text. Synced with the agent.
ALTER TABLE agents ADD COLUMN persona TEXT;
